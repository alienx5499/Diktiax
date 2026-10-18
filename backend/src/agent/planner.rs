//! Planner contract: a planner only DECIDES which graph tools to call next. The investigator
//! executes every call through `crate::tools::execute_tool` and feeds outcomes back.

use serde_json::Value;
use std::future::Future;

/// Hard bound on executed tool calls per investigation, for every planner.
pub const MAX_STEPS: usize = 12;

use crate::tools::{
    DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE, DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING,
    RING_COMPONENTS,
};

pub const DETECTORS: [&str; 4] = [
    DETECT_GHOST_COLLISION,
    DETECT_SHARED_CONTACTS,
    DETECT_VIN_CLONING,
    DETECT_KICKBACK_TRIANGLE,
];

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCall {
    /// Planner-side correlation id (Claude `tool_use.id`); `None` for the rule-based planner.
    pub id: Option<String>,
    pub tool: String,
    pub arguments: Value,
    pub rationale: String,
}

impl PlannedCall {
    pub fn new(tool: &str, arguments: Value, rationale: String) -> Self {
        Self {
            id: None,
            tool: tool.to_string(),
            arguments,
            rationale,
        }
    }

    pub fn claim_arg(&self) -> Option<&str> {
        self.arguments.get("claim_id").and_then(Value::as_str)
    }
}

/// One executed call. Tool errors are kept (not swallowed) so planners can react to them.
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub call: PlannedCall,
    pub result: Result<Value, String>,
    pub latency_ms: f64,
}

pub trait Planner {
    /// Next calls given every outcome so far; an empty Vec means "investigation complete".
    fn plan(
        &mut self,
        claim_id: &str,
        history: &[ToolOutcome],
    ) -> impl Future<Output = Result<Vec<PlannedCall>, String>> + Send;

    /// Final free-text narrative, if the planner produced one (never used for scoring).
    fn narrative(&self) -> Option<&str> {
        None
    }
}

/// algo.WCC labels the whole graph, so `ring_components` runs at most once per investigation,
/// whatever the planner asks. A repeat for the same claim gets the earlier outcome back; one
/// for another claim is refused (the component is per claim, so reusing it would be wrong).
/// `None`: not a repeat, execute the call.
pub fn repeated_ring_component(history: &[ToolOutcome], call: &PlannedCall) -> Option<ToolOutcome> {
    if call.tool != RING_COMPONENTS {
        return None;
    }
    let (i, earlier) = history
        .iter()
        .enumerate()
        .find(|(_, o)| o.call.tool == RING_COMPONENTS)?;
    let step = i + 1;
    let mut call = call.clone();
    let result = if earlier.call.claim_arg() == call.claim_arg() {
        call.rationale = format!("{} (reused from step {step}, not re-run)", call.rationale);
        earlier.result.clone()
    } else {
        Err(format!(
            "ring_components already ran at step {step}; algo.WCC runs once per investigation"
        ))
    };
    Some(ToolOutcome {
        call,
        result,
        latency_ms: 0.0,
    })
}

/// Hides the API key in error text; an empty key leaves the text unchanged.
pub(super) fn redact_key(error: String, key: &str) -> String {
    if key.is_empty() {
        return error;
    }
    error.replace(key, "[redacted]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ring_components_runs_once_per_investigation() {
        let wcc = |claim: &str| {
            PlannedCall::new(RING_COMPONENTS, json!({ "claim_id": claim }), "why".into())
        };
        let first = ToolOutcome {
            call: wcc("C1"),
            result: Ok(json!({ "claims": ["C1", "C2"] })),
            latency_ms: 5.0,
        };
        let explore = PlannedCall::new(
            "explore_claim_subgraph",
            json!({ "claim_id": "C1" }),
            String::new(),
        );
        assert!(
            repeated_ring_component(&[], &wcc("C1")).is_none(),
            "first call executes"
        );
        assert!(repeated_ring_component(std::slice::from_ref(&first), &explore).is_none());

        let history = [
            ToolOutcome {
                call: explore,
                result: Ok(json!({})),
                latency_ms: 1.0,
            },
            first,
        ];
        let again = repeated_ring_component(&history, &wcc("C1")).expect("not re-run");
        assert_eq!(again.result, history[1].result);
        assert_eq!(again.latency_ms, 0.0);
        assert!(
            again.call.rationale.contains("reused from step 2"),
            "{}",
            again.call.rationale
        );

        let other = repeated_ring_component(&history, &wcc("C9")).expect("not run");
        assert!(other.result.unwrap_err().contains("already ran at step 2"));
    }

    #[test]
    fn redact_key_hides_every_occurrence_and_ignores_empty_key() {
        assert_eq!(redact_key("key sk-1 sent".into(), ""), "key sk-1 sent");
        assert_eq!(
            redact_key("sk-1 rejected: sk-1".into(), "sk-1"),
            "[redacted] rejected: [redacted]"
        );
    }
}
