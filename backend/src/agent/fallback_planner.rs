//! Wraps a primary LLM planner (Claude or OpenAI-compatible): on its first error the
//! investigation continues with the deterministic rule-based policy from the CURRENT history,
//! so executed steps are kept.

use super::planner::{PlannedCall, Planner, ToolOutcome};
use super::rule_planner::next_calls;
use tracing::warn;

pub struct FallbackPlanner<P> {
    primary: P,
    /// (public failure reason, history length when the primary failed)
    failure: Option<(&'static str, usize)>,
}

impl<P> FallbackPlanner<P> {
    pub fn new(primary: P) -> Self {
        Self {
            primary,
            failure: None,
        }
    }

    /// `primary` when it never failed, else `rule-based (fallback: <generic reason>)`.
    pub fn label(&self, primary: &str) -> String {
        match &self.failure {
            None => primary.to_string(),
            Some((reason, _)) => format!("rule-based (fallback: {reason})"),
        }
    }

    /// Rationale note recording the failure, if any.
    pub fn failure_note(&self, primary: &str) -> Option<String> {
        self.failure.as_ref().map(|(reason, at)| {
            format!(
                "Planner fallback: {primary} failed after {at} executed step(s) ({reason}); \
                 the rule-based planner continued from the existing history."
            )
        })
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
        if self.failure.is_none() {
            match self.primary.plan(claim_id, history).await {
                Ok(calls) => return Ok(calls),
                Err(e) => {
                    warn!("primary planner failed, falling back to rule-based: {e}");
                    self.failure = Some((public_reason(&e), history.len()));
                }
            }
        }
        let mut calls = next_calls(claim_id, history);
        for call in &mut calls {
            call.rationale = format!("[rule-based fallback] {}", call.rationale);
        }
        Ok(calls)
    }

    fn narrative(&self) -> Option<&str> {
        self.primary.narrative()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::EXPLORE_CLAIM_SUBGRAPH;

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
        let mut p = FallbackPlanner::new(Failing(0));
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
        let mut p = FallbackPlanner::new(FailsWith(groq));
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

    #[test]
    fn healthy_primary_keeps_its_label() {
        let p = FallbackPlanner::new(Failing(0));
        assert_eq!(p.label("claude:m"), "claude:m");
        assert!(p.failure_note("claude:m").is_none());
    }
}
