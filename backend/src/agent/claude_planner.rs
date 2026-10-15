//! Claude-driven planner over the Anthropic Messages API. Claude chooses graph tools from the
//! shared MCP catalog; tool results go back as `tool_result` blocks. Claude never scores.

use super::planner::{PlannedCall, Planner, ToolOutcome};
use serde_json::{json, Value};
use std::time::Duration;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
const MAX_TOKENS: u32 = 2048;

pub(super) const SYSTEM_PROMPT: &str = "You are Diktiax, an insurance fraud investigator working over a FalkorDB \
property graph of claims, customers, phones, addresses, vehicles, VINs, accidents, locations, repair \
shops, clinics and adjusters. Investigate the given claim by calling the graph tools: explore its \
neighbourhood first, then run the fraud detectors the topology suggests, check repair-shop centrality \
for shops implicated by evidence, and follow linked claims when a ring appears. Explain briefly why \
you call each tool. Do not assign risk scores: scoring is computed deterministically from detector \
results. When you have enough evidence, stop calling tools and reply with a short narrative.";

/// Connection settings, read once at startup. Deliberately not `Debug`: it holds the API key.
#[derive(Clone)]
pub struct ClaudeConfig {
    api_key: String,
    pub model: String,
    http: reqwest::Client,
}

impl ClaudeConfig {
    /// `Some` only when `ANTHROPIC_API_KEY` is set and non-blank.
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(api_key) = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
        else {
            return Ok(None);
        };
        let model = std::env::var("ANTHROPIC_MODEL")
            .ok()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Some(Self {
            api_key,
            model,
            http,
        }))
    }
}

/// One investigation's conversation with Claude.
pub struct ClaudePlanner {
    config: ClaudeConfig,
    messages: Vec<Value>,
    /// History length already reported back to Claude as tool results.
    reported: usize,
    narrative: Option<String>,
}

impl ClaudePlanner {
    pub fn new(config: ClaudeConfig) -> Self {
        Self {
            config,
            messages: Vec::new(),
            reported: 0,
            narrative: None,
        }
    }

    /// Appends the opening user turn, or ONE user turn carrying a `tool_result` per new outcome.
    fn absorb(&mut self, claim_id: &str, history: &[ToolOutcome]) {
        if self.messages.is_empty() {
            let text =
                format!("Investigate insurance claim {claim_id} for fraud using the graph tools.");
            self.messages
                .push(json!({ "role": "user", "content": text }));
        }
        let fresh = &history[self.reported.min(history.len())..];
        let results: Vec<Value> = fresh
            .iter()
            .filter_map(|o| {
                let id = o.call.id.as_ref()?;
                let (content, is_error) = match &o.result {
                    Ok(v) => (v.to_string(), false),
                    Err(e) => (e.clone(), true),
                };
                Some(json!({ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error }))
            })
            .collect();
        if !results.is_empty() {
            self.messages
                .push(json!({ "role": "user", "content": results }));
        }
        self.reported = history.len();
    }

    fn request_body(&self) -> Value {
        let tools: Vec<Value> = crate::mcp::tool_definitions()
            .into_iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema }))
            .collect();
        json!({
            "model": self.config.model,
            "max_tokens": MAX_TOKENS,
            "system": SYSTEM_PROMPT,
            "messages": self.messages,
            "tools": tools,
        })
    }

    /// Records the assistant turn verbatim and extracts planned calls (empty = done).
    fn apply_response(&mut self, response: Value) -> Result<Vec<PlannedCall>, String> {
        let content = response
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("Anthropic response has no content array: {response}"))?
            .clone();
        let stop_reason = response
            .get("stop_reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let text: Vec<&str> = content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        let rationale = text.join(" ").trim().to_string();
        self.messages
            .push(json!({ "role": "assistant", "content": content }));

        if stop_reason != "tool_use" {
            self.narrative = Some(rationale).filter(|r| !r.is_empty());
            return Ok(Vec::new());
        }
        let rationale = if rationale.is_empty() {
            "(model gave no rationale text)".to_string()
        } else {
            rationale
        };
        content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .map(|b| {
                let field = |k: &str| {
                    b.get(k)
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("tool_use block missing '{k}': {b}"))
                };
                Ok(PlannedCall {
                    id: Some(field("id")?.to_string()),
                    tool: field("name")?.to_string(),
                    arguments: b.get("input").cloned().unwrap_or_else(|| json!({})),
                    rationale: rationale.clone(),
                })
            })
            .collect()
    }

    /// Errors surface in logs and the dossier label: scrub the API key defensively.
    fn redact(&self, error: String) -> String {
        if self.config.api_key.is_empty() {
            return error;
        }
        error.replace(&self.config.api_key, "[redacted]")
    }

    async fn send(&self, body: &Value) -> Result<Value, String> {
        let response = self
            .config
            .http
            .post(API_URL)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Anthropic API request failed: {}", e.without_url()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("Anthropic API body read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "Anthropic API returned HTTP {}: {}",
                status.as_u16(),
                text.chars().take(500).collect::<String>()
            ));
        }
        serde_json::from_str(&text).map_err(|e| format!("Anthropic API returned invalid JSON: {e}"))
    }
}

impl Planner for ClaudePlanner {
    async fn plan(
        &mut self,
        claim_id: &str,
        history: &[ToolOutcome],
    ) -> Result<Vec<PlannedCall>, String> {
        self.absorb(claim_id, history);
        let body = self.request_body();
        let response = self.send(&body).await.map_err(|e| self.redact(e))?;
        self.apply_response(response).map_err(|e| self.redact(e))
    }

    fn narrative(&self) -> Option<&str> {
        self.narrative.as_deref()
    }
}

#[cfg(test)]
#[path = "claude_planner_tests.rs"]
mod tests;
