use super::*;
use crate::agent::planner::PlannedCall;

fn planner() -> ClaudePlanner {
    ClaudePlanner::new(ClaudeConfig {
        api_key: "test-key".into(),
        model: "m-test".into(),
        http: reqwest::Client::new(),
    })
}

#[test]
fn request_carries_catalog_tools_and_opening_turn() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let body = p.request_body();
    assert_eq!(body["model"], "m-test");
    assert_eq!(body["messages"][0]["role"], "user");
    let tools = body["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), crate::tools::tool_definitions().len());
    assert!(tools.iter().all(|t| t["name"].is_string()
        && t["input_schema"].is_object()
        && t["description"].is_string()));
    assert!(
        !body.to_string().contains("test-key"),
        "API key never goes into the body"
    );
}

#[test]
fn tool_use_blocks_become_calls_and_results_return_in_one_turn() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let response = json!({
        "stop_reason": "tool_use",
        "content": [
            { "type": "text", "text": "Explore first." },
            { "type": "tool_use", "id": "tu_1", "name": "explore_claim_subgraph", "input": { "claim_id": "CLM-1" } },
            { "type": "tool_use", "id": "tu_2", "name": "detect_vin_cloning", "input": { "claim_id": "CLM-1" } }
        ]
    });
    let calls = p.apply_response(response).expect("parsed");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id.as_deref(), Some("tu_1"));
    assert_eq!(calls[1].tool, "detect_vin_cloning");
    assert_eq!(calls[0].rationale, "Explore first.");
    assert_eq!(p.messages[1]["role"], "assistant");
    assert_eq!(
        p.messages[1]["content"][1]["id"], "tu_1",
        "assistant content kept verbatim"
    );

    let history: Vec<ToolOutcome> = vec![
        ToolOutcome {
            call: calls[0].clone(),
            result: Ok(json!({ "nodes": [] })),
            latency_ms: 3.0,
        },
        ToolOutcome {
            call: calls[1].clone(),
            result: Err("db down".into()),
            latency_ms: 4.0,
        },
    ];
    p.absorb("CLM-1", &history);
    assert_eq!(p.messages.len(), 3);
    let results = p.messages[2]["content"].as_array().expect("tool results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["type"], "tool_result");
    assert_eq!(results[0]["tool_use_id"], "tu_1");
    assert_eq!(results[1]["is_error"], true);
    assert_eq!(results[1]["content"], "db down");

    p.absorb("CLM-1", &history);
    assert_eq!(p.messages.len(), 3, "outcomes are reported once");
}

#[test]
fn end_turn_stops_and_keeps_narrative() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let done = json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Ring confirmed." }] });
    assert_eq!(
        p.apply_response(done).expect("parsed"),
        Vec::<PlannedCall>::new()
    );
    assert_eq!(p.narrative(), Some("Ring confirmed."));
}

#[test]
fn malformed_response_is_an_error() {
    let mut p = planner();
    assert!(p.apply_response(json!({ "type": "error" })).is_err());
    let bad =
        json!({ "stop_reason": "tool_use", "content": [{ "type": "tool_use", "input": {} }] });
    assert!(p.apply_response(bad).is_err());
}

#[test]
fn errors_never_carry_the_api_key() {
    let p = planner();
    assert_eq!(
        redact_key("HTTP 401: bad key test-key".into(), &p.config.api_key),
        "HTTP 401: bad key [redacted]"
    );
}
