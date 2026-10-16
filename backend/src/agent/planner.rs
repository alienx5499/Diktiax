//! Planner contract: a planner only DECIDES which graph tools to call next. The investigator
//! executes every call through `crate::tools::execute_tool` and feeds outcomes back.

use serde_json::Value;
use std::future::Future;

/// Hard bound on executed tool calls per investigation, for every planner.
pub const MAX_STEPS: usize = 12;

use crate::tools::{
    DETECT_GHOST_COLLISION, DETECT_KICKBACK_TRIANGLE, DETECT_SHARED_CONTACTS, DETECT_VIN_CLONING,
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
