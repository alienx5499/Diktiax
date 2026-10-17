use super::*;

fn config(base_url: &str) -> OpenAiConfig {
    OpenAiConfig::new(
        "gsk-test-key".into(),
        "m-test".into(),
        base_url.into(),
        reqwest::Client::new(),
    )
}

fn planner() -> OpenAiPlanner {
    OpenAiPlanner::new(config(DEFAULT_BASE_URL))
}

fn tool_call(id: &str, name: &str, arguments: &str) -> Value {
    json!({ "id": id, "type": "function", "function": { "name": name, "arguments": arguments } })
}

fn tool_response(calls: Vec<Value>) -> Value {
    json!({ "choices": [{ "finish_reason": "tool_calls", "message": {
        "role": "assistant", "content": "Explore first.", "tool_calls": calls } }] })
}

#[test]
fn request_maps_catalog_to_openai_function_tools() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let body = p.request_body();
    assert_eq!(body["model"], "m-test");
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    let tools = body["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), crate::tools::tool_definitions().len());
    assert!(tools.iter().all(|t| t["type"] == "function"
        && t["function"]["name"].is_string()
        && t["function"]["description"].is_string()
        && t["function"]["parameters"].is_object()));
    assert!(!body.to_string().contains("gsk-test-key"));
}

#[test]
fn stringified_arguments_become_calls_and_results_match_ids() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let response = tool_response(vec![
        tool_call(
            "call_1",
            "explore_claim_subgraph",
            r#"{"claim_id":"CLM-1"}"#,
        ),
        tool_call("call_2", "detect_vin_cloning", r#"{"claim_id":"CLM-1"}"#),
    ]);
    let calls = p.apply_response(response).expect("parsed");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id.as_deref(), Some("call_1"));
    assert_eq!(calls[0].arguments, json!({ "claim_id": "CLM-1" }));
    assert_eq!(calls[1].tool, "detect_vin_cloning");
    assert_eq!(calls[0].rationale, "Explore first.");
    assert_eq!(p.messages[2]["role"], "assistant");
    assert_eq!(
        p.messages[2]["tool_calls"][0]["id"], "call_1",
        "verbatim assistant message"
    );

    let history = vec![
        ToolOutcome {
            call: calls[0].clone(),
            result: Ok(json!({ "nodes": [], "edges": [] })),
            latency_ms: 1.0,
        },
        ToolOutcome {
            call: calls[1].clone(),
            result: Err("db down".into()),
            latency_ms: 1.0,
        },
    ];
    p.absorb("CLM-1", &history);
    assert_eq!(p.messages.len(), 5);
    assert_eq!(p.messages[3]["role"], "tool");
    assert_eq!(p.messages[3]["tool_call_id"], "call_1");
    assert_eq!(p.messages[3]["content"], "0 nodes, 0 edges: ");
    assert_eq!(p.messages[4]["tool_call_id"], "call_2");
    assert_eq!(p.messages[4]["content"], "ERROR: db down");

    p.absorb("CLM-1", &history);
    assert_eq!(p.messages.len(), 5, "outcomes are reported once");
}

#[test]
fn malformed_arguments_are_fed_back_not_executed() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let response = tool_response(vec![
        tool_call("call_bad", "detect_vin_cloning", "{claim_id: CLM-1"),
        tool_call("call_ok", "get_claim", r#"{"claim_id":"CLM-1"}"#),
    ]);
    let calls = p.apply_response(response).expect("no panic, no error");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id.as_deref(), Some("call_ok"));
    assert!(!p.needs_repair);
    let fed_back = &p.messages[3];
    assert_eq!(fed_back["role"], "tool");
    assert_eq!(fed_back["tool_call_id"], "call_bad");
    assert!(fed_back["content"]
        .as_str()
        .is_some_and(|c| c.starts_with("error: tool arguments are not valid JSON")));

    let all_bad = tool_response(vec![tool_call("call_x", "get_claim", "[1]")]);
    assert!(p.apply_response(all_bad).expect("parsed").is_empty());
    assert!(p.needs_repair, "all-rejected turn asks the model again");
}

#[test]
fn no_tool_calls_stops_and_keeps_narrative() {
    let mut p = planner();
    p.absorb("CLM-1", &[]);
    let done = json!({ "choices": [{ "finish_reason": "stop",
        "message": { "role": "assistant", "content": "Ring confirmed." } }] });
    assert!(p.apply_response(done).expect("parsed").is_empty());
    assert!(!p.needs_repair);
    assert_eq!(p.narrative(), Some("Ring confirmed."));
}

#[test]
fn malformed_response_is_an_error() {
    let mut p = planner();
    assert!(p.apply_response(json!({ "error": "x" })).is_err());
    let no_id = tool_response(vec![json!({ "function": { "name": "get_claim" } })]);
    assert!(p.apply_response(no_id).is_err());
}

#[test]
fn errors_never_carry_the_api_key() {
    let p = planner();
    assert_eq!(
        p.redact("HTTP 401: bad key gsk-test-key".into()),
        "HTTP 401: bad key [redacted]"
    );
}

#[test]
fn label_names_groq_only_on_the_default_endpoint() {
    assert_eq!(config(DEFAULT_BASE_URL).label(), "groq:m-test");
    assert_eq!(
        config("https://api.groq.com/openai/v1/").label(),
        "groq:m-test"
    );
    assert_eq!(
        config("http://localhost:11434/v1").label(),
        "openai-compatible:m-test"
    );
}

#[test]
fn model_sees_compact_node_list_not_raw_json() {
    let call = PlannedCall::new(
        "explore_claim_subgraph",
        json!({ "claim_id": "C" }),
        String::new(),
    );
    let nodes: Vec<Value> = (0..400)
        .map(|i| json!({ "id": format!("N-{i}"), "label": "Claim", "title": "", "group": "", "properties": {} }))
        .collect();
    let outcome = ToolOutcome {
        call,
        result: Ok(json!({ "nodes": nodes, "edges": [] })),
        latency_ms: 1.0,
    };
    let text = crate::agent::evidence::compact(&outcome);
    assert!(text.starts_with("400 nodes, 0 edges: Claim:N-0, Claim:N-1"));
    assert!(
        !text.contains("properties"),
        "raw JSON must not reach the model"
    );
    assert!(text.chars().count() <= 1503, "size is capped");
}

#[test]
fn reasoning_is_the_rationale_when_content_is_empty() {
    let mut p = planner();
    let response = json!({ "choices": [{ "finish_reason": "tool_calls", "message": {
        "role": "assistant", "content": "", "reasoning": "Check VINs.",
        "tool_calls": [tool_call("c1", "detect_vin_cloning", r#"{"claim_id":"C"}"#)] } }] });
    let calls = p.apply_response(response).expect("parsed");
    assert_eq!(calls[0].rationale, "Check VINs.");
}

#[test]
fn groq_tool_validation_400_is_retryable_but_other_errors_are_not() {
    let groq = r#"LLM API returned HTTP 400: {"error":{"message":"Tool call validation failed: attempted to call tool 'detect_kickback_triangle<...'","code":"tool_use_failed"}}"#;
    assert!(is_tool_call_rejection(groq));
    assert!(!is_tool_call_rejection(
        "LLM API returned HTTP 401: invalid api key"
    ));
    assert!(!is_tool_call_rejection(
        "LLM API returned HTTP 400: context length exceeded"
    ));
}

#[test]
fn rate_limit_waits_fail_over_on_daily_quota_or_spent_budget() {
    assert!(within_wait_budget(9, 0), "per-minute limit: wait");
    assert!(within_wait_budget(16, 29), "still inside the 45 s budget");
    assert!(
        !within_wait_budget(3600, 0),
        "daily quota: fail over at once"
    );
    assert!(!within_wait_budget(20, 30), "budget spent: fail over");
}
