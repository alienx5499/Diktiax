//! Wraps a primary LLM planner (Claude or OpenAI-compatible): on its first error an optional
//! backup planner (second provider) takes over, and on the backup's error (or with none) the
//! deterministic rule-based policy does. Each continues from the CURRENT history, so executed
//! steps are kept.

use super::planner::{PlannedCall, Planner, ToolOutcome};
use super::rule_planner::RuleBasedPlanner;
use tracing::warn;

/// (public failure reason, history length when the planner failed)
type Failure = (&'static str, usize);

pub struct FallbackPlanner<P> {
    primary: P,
    backup: Option<P>,
    /// Set when the primary failed and the backup took over.
    handover: Option<Failure>,
    /// Set when the rule-based planner took over.
    failure: Option<Failure>,
    /// Rule-based fallback; also applies the memory filter to every planner's calls.
    rule: RuleBasedPlanner,
}

impl<P> FallbackPlanner<P> {
    pub fn new(primary: P, backup: Option<P>) -> Self {
        Self {
            primary,
            backup,
            handover: None,
            failure: None,
            rule: RuleBasedPlanner::default(),
        }
    }

    /// Skip corroboration on linked claims the latest prior verdict relied on, whoever plans.
    pub fn remembering(mut self, remembered: Vec<String>) -> Self {
        self.rule = RuleBasedPlanner::remembering(remembered);
        self
    }

    pub fn skipped(&self) -> &[PlannedCall] {
        self.rule.skipped()
    }

    /// `primary` when it never failed, `<primary> (backup provider after: <reason>)` while the
    /// backup plans, else `rule-based (fallback: <generic reason>)`.
    pub fn label(&self, primary: &str) -> String {
        match (&self.handover, &self.failure) {
            (_, Some((reason, _))) => format!("rule-based (fallback: {reason})"),
            (Some((reason, _)), None) => format!("{primary} (backup provider after: {reason})"),
            (None, None) => primary.to_string(),
        }
    }

    /// Rationale note recording the failures, if any.
    pub fn failure_note(&self, primary: &str) -> Option<String> {
        let failed =
            |(reason, at): &Failure| format!("failed after {at} executed step(s) ({reason})");
        let note = match (&self.handover, &self.failure) {
            (None, None) => return None,
            (Some(h), None) => format!("{primary} {}; the backup provider continued", failed(h)),
            (None, Some(f)) => format!("{primary} {}; the rule-based planner continued", failed(f)),
            (Some(h), Some(f)) => format!(
                "{primary} {}; the backup provider {}; the rule-based planner continued",
                failed(h),
                failed(f)
            ),
        };
        Some(format!(
            "Planner fallback: {note} from the existing history."
        ))
    }
}

/// Generic, public-safe reason for a planner error. Raw provider errors carry response bodies
/// (org ids, quota details) and must never reach the API; they are only logged server-side.
fn public_reason(error: &str) -> &'static str {
    if error.contains("HTTP 429") {
        "LLM rate-limited"
    } else if error.contains("timed out") {
        "LLM timed out"
    } else {
        "LLM unavailable"
    }
}

impl<P: Planner + Send> Planner for FallbackPlanner<P> {
    async fn plan(
        &mut self,
        claim_id: &str,
        history: &[ToolOutcome],
    ) -> Result<Vec<PlannedCall>, String> {
        while self.failure.is_none() {
            let (planner, on_backup) = match (&self.handover, &mut self.backup) {
                (Some(_), Some(backup)) => (backup, true),
                _ => (&mut self.primary, false),
            };
            match planner.plan(claim_id, history).await {
                Ok(mut calls) if on_backup => {
                    for call in &mut calls {
                        call.rationale = format!("[backup provider] {}", call.rationale);
                    }
                    return Ok(self.rule.skip_remembered(claim_id, calls));
                }
                Ok(calls) => return Ok(self.rule.skip_remembered(claim_id, calls)),
                Err(e) if on_backup || self.backup.is_none() => {
                    warn!("LLM planner failed, falling back to rule-based: {e}");
                    self.failure = Some((public_reason(&e), history.len()));
                }
                Err(e) => {
                    warn!("primary planner failed, switching to the backup provider: {e}");
                    self.handover = Some((public_reason(&e), history.len()));
                }
            }
        }
        let mut calls = self.rule.plan(claim_id, history).await?;
        for call in &mut calls {
            call.rationale = format!("[rule-based fallback] {}", call.rationale);
        }
        Ok(calls)
    }

    fn narrative(&self) -> Option<&str> {
        match (&self.handover, &self.backup) {
            (Some(_), Some(backup)) => backup.narrative(),
            _ => self.primary.narrative(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::EXPLORE_CLAIM_SUBGRAPH;
    use serde_json::json;

    /// Primary that errors on the first call, like an Anthropic HTTP failure.
    struct Failing(usize);

    impl Planner for Failing {
        async fn plan(&mut self, _: &str, _: &[ToolOutcome]) -> Result<Vec<PlannedCall>, String> {
            self.0 += 1;
            Err("Anthropic API returned HTTP 529:\n  {\"type\":\"overloaded_error\"}".into())
        }
    }

    #[tokio::test]
    async fn primary_error_falls_back_to_rule_based_loudly() {
        let mut p = FallbackPlanner::new(Failing(0), None);
        let calls = p.plan("C1", &[]).await.expect("fallback never errors");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, EXPLORE_CLAIM_SUBGRAPH);
        assert!(calls[0].rationale.starts_with("[rule-based fallback]"));
        assert_eq!(
            p.label("claude:m"),
            "rule-based (fallback: LLM unavailable)"
        );
        assert!(p.failure_note("claude:m").is_some());

        p.plan("C1", &[]).await.expect("still falls back");
        assert_eq!(p.primary.0, 1, "failed primary is not asked again");
    }

    /// Primary that fails with a fixed error.
    struct FailsWith(&'static str);

    impl Planner for FailsWith {
        async fn plan(&mut self, _: &str, _: &[ToolOutcome]) -> Result<Vec<PlannedCall>, String> {
            Err(self.0.into())
        }
    }

    #[tokio::test]
    async fn rate_limit_body_never_reaches_label_or_rationale() {
        let groq = r#"LLM API returned HTTP 429: {"error":{"message":"Rate limit reached for model in organization org_abc123 on tokens per day"}}"#;
        let mut p = FallbackPlanner::new(FailsWith(groq), None);
        let calls = p.plan("C1", &[]).await.expect("fallback never errors");
        let label = p.label("openai:llama");
        let note = p.failure_note("openai:llama").expect("failure recorded");
        assert_eq!(label, "rule-based (fallback: LLM rate-limited)");
        for text in [label.as_str(), note.as_str(), calls[0].rationale.as_str()] {
            assert!(!text.contains("org_"), "org id leaked: {text}");
            assert!(!text.contains('{'), "raw body leaked: {text}");
        }
    }

    #[test]
    fn errors_map_to_one_generic_reason_per_class() {
        assert_eq!(
            public_reason("LLM API returned HTTP 429: {}"),
            "LLM rate-limited"
        );
        assert_eq!(
            public_reason("LLM API request failed: error sending request: operation timed out"),
            "LLM timed out"
        );
        assert_eq!(
            public_reason("LLM API returned HTTP 401: invalid api key"),
            "LLM unavailable"
        );
    }

    /// Planner that either always fails with `error` or returns one call; records the history
    /// length it was asked with.
    struct Scripted {
        error: Option<&'static str>,
        seen: Vec<usize>,
    }

    impl Scripted {
        fn new(error: Option<&'static str>) -> Self {
            Self {
                error,
                seen: Vec::new(),
            }
        }
    }

    impl Planner for Scripted {
        async fn plan(&mut self, c: &str, h: &[ToolOutcome]) -> Result<Vec<PlannedCall>, String> {
            self.seen.push(h.len());
            match self.error {
                Some(e) => Err(e.into()),
                None => Ok(vec![PlannedCall::new(
                    "get_claim",
                    json!({ "claim_id": c }),
                    "LLM".into(),
                )]),
            }
        }
    }

    /// Planner that asks for one detector on a linked claim and one on the target.
    struct Corroborating;

    impl Planner for Corroborating {
        async fn plan(&mut self, c: &str, _: &[ToolOutcome]) -> Result<Vec<PlannedCall>, String> {
            Ok(vec![
                PlannedCall::new(
                    "detect_ghost_collision",
                    json!({ "claim_id": "L1" }),
                    "LLM".into(),
                ),
                PlannedCall::new(
                    "detect_ghost_collision",
                    json!({ "claim_id": c }),
                    "LLM".into(),
                ),
            ])
        }
    }

    #[tokio::test]
    async fn llm_calls_on_remembered_linked_claims_are_skipped() {
        let mut p = FallbackPlanner::new(Corroborating, None).remembering(vec!["L1".into()]);
        let calls = p.plan("C1", &[]).await.expect("plans");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].claim_arg(), Some("C1"));
        p.plan("C1", &[]).await.expect("plans");
        assert_eq!(p.skipped().len(), 1, "a repeat skip is recorded once");
        assert_eq!(p.skipped()[0].claim_arg(), Some("L1"));
    }

    const GROQ_429: &str = r#"LLM API returned HTTP 429: {"error":{"message":"Rate limit reached in organization org_abc123"}} key [redacted]"#;

    fn executed(n: usize) -> Vec<ToolOutcome> {
        (0..n)
            .map(|_| ToolOutcome {
                call: PlannedCall::new(
                    EXPLORE_CLAIM_SUBGRAPH,
                    json!({ "claim_id": "C1" }),
                    String::new(),
                ),
                result: Ok(json!({})),
                latency_ms: 1.0,
            })
            .collect()
    }

    #[tokio::test]
    async fn rate_limited_primary_hands_over_to_backup_from_current_history() {
        let mut p = FallbackPlanner::new(Scripted::new(Some(GROQ_429)), Some(Scripted::new(None)));
        let history = executed(2);
        let calls = p.plan("C1", &history).await.expect("backup plans");
        p.plan("C1", &executed(3))
            .await
            .expect("backup keeps planning");
        assert_eq!(calls[0].rationale, "[backup provider] LLM");
        assert_eq!(p.primary.seen, vec![2], "failed primary is not asked again");
        assert_eq!(
            p.backup.as_ref().map(|b| b.seen.clone()),
            Some(vec![2, 3]),
            "backup sees the full history"
        );
        let label = p.label("groq:m");
        let note = p.failure_note("groq:m").expect("handover recorded");
        assert_eq!(label, "groq:m (backup provider after: LLM rate-limited)");
        assert!(
            note.contains("failed after 2 executed step(s)")
                && note.contains("backup provider continued")
        );
        for text in [label.as_str(), note.as_str(), calls[0].rationale.as_str()] {
            assert!(
                !text.contains("org_") && !text.contains('{') && !text.contains("redacted"),
                "leak: {text}"
            );
        }
    }

    #[tokio::test]
    async fn both_providers_failing_leaves_rules_to_finish() {
        let backup = Scripted::new(Some("LLM API returned HTTP 429: {}"));
        let mut p = FallbackPlanner::new(Scripted::new(Some(GROQ_429)), Some(backup));
        let calls = p.plan("C1", &[]).await.expect("rules never error");
        assert_eq!(calls[0].tool, EXPLORE_CLAIM_SUBGRAPH);
        assert!(calls[0].rationale.starts_with("[rule-based fallback]"));
        p.plan("C1", &executed(1)).await.expect("rules keep going");
        assert_eq!(
            p.backup.as_ref().map(|b| b.seen.len()),
            Some(1),
            "failed backup is not asked again"
        );
        assert_eq!(p.label("groq:m"), "rule-based (fallback: LLM rate-limited)");
        let note = p.failure_note("groq:m").expect("failures recorded");
        assert!(
            note.contains("the backup provider failed")
                && note.contains("rule-based planner continued")
        );
        assert!(!note.contains("org_"));
    }

    #[test]
    fn healthy_primary_keeps_its_label() {
        let p = FallbackPlanner::new(Failing(0), None);
        assert_eq!(p.label("claude:m"), "claude:m");
        assert!(p.failure_note("claude:m").is_none());
    }
}
