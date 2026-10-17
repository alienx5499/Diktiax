//! Wire-format helpers for the OpenAI-compatible planner: argument parsing, rate-limit
//! back-off and recognising a provider's rejection of a malformed tool call.

use serde_json::Value;

/// A 429 asking for longer than this is not worth waiting for in an interactive demo: fail over.
const MAX_RETRY_WAIT_SECS: u64 = 10;
/// Total 429 waiting per investigation. Kept short so a rate-limited demo falls back to the
/// rule-based planner within seconds instead of making the user wait.
const MAX_TOTAL_WAIT_SECS: u64 = 10;

/// Whether waiting `wait` more seconds (after `waited` already) stays inside the budget.
pub(super) fn within_wait_budget(wait: u64, waited: u64) -> bool {
    wait <= MAX_RETRY_WAIT_SECS && waited + wait <= MAX_TOTAL_WAIT_SECS
}

/// Seconds from a `retry-after` header (default 5 when absent or unparseable).
pub(super) fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> u64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(5, |s| s.ceil().max(1.0) as u64)
}

pub(super) const TOOL_CALL_RETRY: &str =
    "Your last tool call was rejected as malformed. Call one of the listed tools by its exact name.";

pub(super) fn is_tool_call_rejection(error: &str) -> bool {
    error.contains("HTTP 400")
        && (error.contains("tool_use_failed") || error.contains("Tool call validation failed"))
}

/// `function.arguments` is a JSON *string* that must decode to an object.
pub(super) fn parse_arguments(raw: &str) -> Result<Value, String> {
    let raw = if raw.trim().is_empty() { "{}" } else { raw };
    match serde_json::from_str::<Value>(raw) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(v) => Err(format!("tool arguments must be a JSON object, got: {v}")),
        Err(e) => Err(format!("tool arguments are not valid JSON ({e}): {raw}")),
    }
}
