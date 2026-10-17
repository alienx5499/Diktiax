//! Wraps a primary LLM planner (Claude or OpenAI-compatible): on its first error the
//! investigation continues with the deterministic rule-based policy from the CURRENT history,
//! so executed steps are kept.

use super::planner::{PlannedCall, Planner, ToolOutcome};
use super::rule_planner::next_calls;
use tracing::warn;

/// Longest planner error kept in the dossier label.
const MAX_ERROR_CHARS: usize = 160;

pub struct FallbackPlanner<P> {
    primary: P,
    /// (shortened error, history length when the primary failed)
    failure: Option<(String, usize)>,
}

impl<P> FallbackPlanner<P> {
    pub fn new(primary: P) -> Self {
        Self {
            primary,
            failure: None,
        }
    }

    /// `primary` when it never failed, else `rule-based (fallback: <primary> failed: <error>)`.
    pub fn label(&self, primary: &str) -> String {
        match &self.failure {
            None => primary.to_string(),
            Some((error, _)) => {
                format!("rule-based (fallback: {primary} failed: {})", brief(error))
            }
        }
    }

    /// Rationale note recording the failure, if any.
    pub fn failure_note(&self, primary: &str) -> Option<String> {
        self.failure.as_ref().map(|(error, at)| {
            format!(
                "Planner fallback: {primary} failed after {at} executed step(s) ({error}); \
                 the rule-based planner continued from the existing history."
            )
        })
    }
}

/// One line, bounded length. Callers pass errors already scrubbed of secrets.
fn shorten(error: &str) -> String {
    let line = error.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(MAX_ERROR_CHARS) {
        Some((i, _)) => format!("{}...", &line[..i]),
        None => line,
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
                    let error = shorten(&e);
                    warn!("primary planner failed, falling back to rule-based: {error}");
                    self.failure = Some((error, history.len()));
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

/// First 80 characters of an error, for one-line labels.
fn brief(error: &str) -> String {
    const MAX: usize = 80;
    match error.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &error[..cut]),
        None => error.to_string(),
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
            "rule-based (fallback: claude:m failed: Anthropic API returned HTTP 529: {\"type\":\"overloaded_error\"})"
        );
        assert!(p.failure_note("claude:m").is_some());

        let long = format!("LLM API returned HTTP 429: {}", "x".repeat(400));
        assert_eq!(
            brief(&long).chars().count(),
            81,
            "badge label stays one line"
        );

        p.plan("C1", &[]).await.expect("still falls back");
        assert_eq!(p.primary.0, 1, "failed primary is not asked again");
    }

    #[test]
    fn healthy_primary_keeps_its_label_and_long_errors_are_cut() {
        let p = FallbackPlanner::new(Failing(0));
        assert_eq!(p.label("claude:m"), "claude:m");
        assert!(p.failure_note("claude:m").is_none());
        assert_eq!(shorten(&"x".repeat(500)).len(), MAX_ERROR_CHARS + 3);
    }
}
