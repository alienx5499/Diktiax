//! Pure JSON-RPC 2.0 / MCP message routing. Classifies one inbound line and builds every
//! response that does not need the graph; tool execution is left to the caller (`stdio.rs`).

use crate::tools::{investigate_claim_definition, tool_definitions, ToolDefinition};
use serde_json::{json, Map, Value};

pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] =
    [LATEST_PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str =
    "Diktiax exposes a FalkorDB insurance-claim knowledge graph. Start with \
    get_claim, run the detect_* tools to test specific fraud-ring patterns, use list_claim_shops + \
    shop_centrality for repair-shop hubs, explore_claim_subgraph for raw context, or \
    investigate_claim for a full autonomous verdict.";

/// What to do with one inbound line.
#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    /// Send this response as-is.
    Respond(Value),
    /// Notification or client response: send nothing.
    Silent,
    /// Execute a tool, then answer with `tool_result(id, ..)`.
    CallTool {
        id: Value,
        name: String,
        arguments: Value,
    },
}

pub fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// MCP tool result. A failing tool is reported in-band (`isError: true`), not as a JSON-RPC error.
pub fn tool_result(id: Value, outcome: Result<Value, String>) -> Value {
    let (text, is_error) = match outcome {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(text) => (text, false),
            Err(e) => (format!("failed to serialize tool result: {e}"), true),
        },
        Err(message) => (message, true),
    };
    success(
        id,
        json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
    )
}

fn mcp_tool(def: ToolDefinition) -> Value {
    json!({ "name": def.name, "description": def.description, "inputSchema": def.input_schema })
}

/// Graph catalog plus the MCP-only `investigate_claim` tool.
pub fn list_tools() -> Value {
    let tools: Vec<Value> = tool_definitions()
        .into_iter()
        .chain(std::iter::once(investigate_claim_definition()))
        .map(mcp_tool)
        .collect();
    json!({ "tools": tools })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| SUPPORTED_PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(LATEST_PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "diktiax", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tools_call(id: Value, params: &Value) -> Route {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Route::Respond(error(
            id,
            INVALID_PARAMS,
            "tools/call requires a string 'name'",
        ));
    };
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(args @ Value::Object(_)) => args.clone(),
        Some(_) => {
            return Route::Respond(error(
                id,
                INVALID_PARAMS,
                "tools/call 'arguments' must be an object",
            ))
        }
    };
    Route::CallTool {
        id,
        name: name.to_string(),
        arguments,
    }
}

/// Routes one parsed JSON-RPC message.
pub fn route(message: &Value) -> Route {
    let Some(obj) = message.as_object() else {
        return Route::Respond(error(
            Value::Null,
            INVALID_REQUEST,
            "request must be a JSON object",
        ));
    };
    let id = match obj.get("id") {
        None => None,
        Some(id @ (Value::String(_) | Value::Number(_) | Value::Null)) => Some(id.clone()),
        Some(_) => {
            return Route::Respond(error(
                Value::Null,
                INVALID_REQUEST,
                "id must be a string or number",
            ))
        }
    };
    let reply_id = id.clone().unwrap_or(Value::Null);
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Route::Respond(error(reply_id, INVALID_REQUEST, "jsonrpc must be \"2.0\""));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // A response from the client (we never send requests) is ignored; anything else is invalid.
        if obj.contains_key("result") || obj.contains_key("error") {
            return Route::Silent;
        }
        return Route::Respond(error(reply_id, INVALID_REQUEST, "method must be a string"));
    };
    let Some(id) = id else {
        return Route::Silent; // notification: never answered
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    if !matches!(params, Value::Null | Value::Object(_)) {
        return Route::Respond(error(id, INVALID_PARAMS, "params must be an object"));
    }
    match method {
        "initialize" => Route::Respond(success(id, initialize(&params))),
        "ping" => Route::Respond(success(id, json!({}))),
        "tools/list" => Route::Respond(success(id, list_tools())),
        "tools/call" => tools_call(id, &params),
        other => Route::Respond(error(
            id,
            METHOD_NOT_FOUND,
            format!("method '{other}' not found"),
        )),
    }
}

/// Parses then routes one raw line; invalid JSON yields a parse error with a null id.
pub fn route_line(line: &str) -> Route {
    match serde_json::from_str::<Value>(line) {
        Ok(message) => route(&message),
        Err(e) => Route::Respond(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn respond(line: &str) -> Value {
        match route_line(line) {
            Route::Respond(v) => v,
            other => panic!("expected a response, got {other:?}"),
        }
    }

    #[test]
    fn initialize_echoes_supported_version_and_advertises_tools() {
        let r = respond(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        );
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], false);
        assert_eq!(r["result"]["serverInfo"]["name"], "diktiax");
        let r = respond(
            r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(r["id"], "a");
        assert_eq!(r["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn notifications_and_client_responses_are_silent() {
        assert_eq!(
            route_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            Route::Silent
        );
        assert_eq!(
            route_line(r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"get_claim"}}"#),
            Route::Silent
        );
        assert_eq!(
            route_line(r#"{"jsonrpc":"2.0","id":5,"result":{}}"#),
            Route::Silent
        );
    }

    #[test]
    fn ping_returns_empty_object() {
        assert_eq!(
            respond(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#)["result"],
            json!({})
        );
    }

    #[test]
    fn protocol_errors_use_spec_codes() {
        let r = respond("garbage");
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        assert!(r.get("id").is_some_and(Value::is_null));
        assert_eq!(respond("[1]")["error"]["code"], INVALID_REQUEST);
        assert_eq!(
            respond(r#"{"id":1,"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        assert_eq!(
            respond(r#"{"jsonrpc":"2.0","id":{},"method":"ping"}"#)["error"]["code"],
            INVALID_REQUEST
        );
        let r = respond(r#"{"jsonrpc":"2.0","id":9,"method":"resources/list"}"#);
        assert_eq!(
            (r["id"].clone(), r["error"]["code"].clone()),
            (json!(9), json!(METHOD_NOT_FOUND))
        );
    }

    #[test]
    fn tools_call_param_validation() {
        let r =
            respond(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"arguments":{}}}"#);
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        let r = respond(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_claim","arguments":[1]}}"#,
        );
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
        assert_eq!(
            route_line(
                r#"{"jsonrpc":"2.0","id":"x","method":"tools/call","params":{"name":"get_claim"}}"#
            ),
            Route::CallTool {
                id: json!("x"),
                name: "get_claim".into(),
                arguments: json!({})
            }
        );
    }

    #[test]
    fn tools_list_has_nine_tools_with_strict_schemas() {
        let r = respond(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools = r["result"]["tools"].as_array().cloned().unwrap_or_default();
        assert_eq!(tools.len(), 9);
        assert!(tools.iter().any(|t| t["name"] == "investigate_claim"));
        for t in &tools {
            let schema = &t["inputSchema"];
            assert_eq!(schema["type"], "object", "{}", t["name"]);
            assert_eq!(schema["additionalProperties"], false);
            assert!(schema["properties"].is_object() && schema["required"].is_array());
            assert!(t["description"].as_str().is_some_and(|d| d.len() > 40));
        }
    }

    #[test]
    fn tool_result_reports_errors_in_band() {
        let ok = tool_result(json!(1), Ok(Value::Null));
        assert_eq!(
            (
                ok["result"]["isError"].clone(),
                ok["result"]["content"][0]["text"].clone()
            ),
            (json!(false), json!("null"))
        );
        let err = tool_result(json!(1), Err("unknown tool 'nope'".into()));
        assert_eq!(err["result"]["isError"], true);
        assert!(err.get("error").is_none());
    }
}
