//! OpenAI-compatible chat-completions planner (Groq by default; Gemini's OpenAI endpoint,
//! OpenRouter or Ollama via `LLM_BASE_URL`). The model only chooses graph tools; it never scores.

use super::claude_planner::SYSTEM_PROMPT;
use super::evidence::compact;
use super::planner::{PlannedCall, Planner, ToolOutcome};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::warn;

const DEFAULT_BASE_URL: &str = "https://api.groq.com/openai/v1";
const DEFAULT_MODEL: &str = "openai/gpt-oss-120b";
/// Extra round trips allowed when every tool call in a turn had unparseable arguments.
const MAX_REPAIRS: usize = 2;
/// HTTP 429 handling: wait `retry-after` (capped) and retry, at most this many times.
const MAX_RATE_LIMIT_RETRIES: usize = 3;
const MAX_RETRY_WAIT_SECS: u64 = 30;

/// Connection settings, read once at startup. Deliberately not `Debug`: it holds the API key.
#[derive(Clone)]
pub struct OpenAiConfig {
    api_key: String,
    pub model: String,
    base_url: String,
    http: reqwest::Client,
}

fn env_non_blank(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl OpenAiConfig {
    /// `Some` only when `GROQ_API_KEY` is set and non-blank.
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(api_key) = env_non_blank("GROQ_API_KEY") else {
            return Ok(None);
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Some(Self::new(
            api_key,
            env_non_blank("GROQ_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            env_non_blank("LLM_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            http,
        )))
    }

    fn new(api_key: String, model: String, base_url: String, http: reqwest::Client) -> Self {
        Self {
            api_key,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    /// `groq:<model>` on the default endpoint, otherwise `openai-compatible:<model>`.
    pub fn label(&self) -> String {
        if self.base_url == DEFAULT_BASE_URL {
            format!("groq:{}", self.model)
        } else {
            format!("openai-compatible:{}", self.model)
        }
    }
}

/// One investigation's chat-completions conversation.
pub struct OpenAiPlanner {
    config: OpenAiConfig,
    messages: Vec<Value>,
    /// History length already reported back as `tool` messages.
    reported: usize,
    /// Set when the last turn's tool calls were all rejected (bad arguments): ask again.
    needs_repair: bool,
    narrative: Option<String>,
}

impl OpenAiPlanner {
    pub fn new(config: OpenAiConfig) -> Self {
        Self {
            config,
            messages: Vec::new(),
            reported: 0,
            needs_repair: false,
            narrative: None,
        }
    }

    /// Appends the opening turns, or one `tool` message per new outcome.
    fn absorb(&mut self, claim_id: &str, history: &[ToolOutcome]) {
        if self.messages.is_empty() {
            let text =
                format!("Investigate insurance claim {claim_id} for fraud using the graph tools.");
            self.messages
                .push(json!({ "role": "system", "content": SYSTEM_PROMPT }));
            self.messages
                .push(json!({ "role": "user", "content": text }));
        }
        let fresh = &history[self.reported.min(history.len())..];
        for outcome in fresh {
            let Some(id) = outcome.call.id.as_ref() else {
                continue;
            };
            self.push_tool_message(id, compact(outcome));
        }
        self.reported = history.len();
    }

    fn push_tool_message(&mut self, id: &str, content: String) {
        self.messages
            .push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
    }

    fn request_body(&self) -> Value {
        let tools: Vec<Value> = crate::tools::tool_definitions()
            .into_iter()
            .map(|t| {
                json!({ "type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.input_schema } })
            })
            .collect();
        json!({
            "model": self.config.model,
            "messages": self.messages,
            "tools": tools,
            "tool_choice": "auto",
            "temperature": 0,
        })
    }

    /// Records the assistant message verbatim and extracts planned calls (empty = done).
    /// Calls with unparseable `arguments` are answered immediately with an error `tool` message.
    fn apply_response(&mut self, response: Value) -> Result<Vec<PlannedCall>, String> {
        let choice = response
            .pointer("/choices/0")
            .ok_or_else(|| format!("chat completion has no choices: {response}"))?;
        let message = choice
            .get("message")
            .filter(|m| m.is_object())
            .ok_or_else(|| format!("chat completion choice has no message: {choice}"))?
            .clone();
        let finish_reason = choice["finish_reason"].as_str().unwrap_or_default();
        // `Value` indexing never panics (missing = Null). gpt-oss may explain in `reasoning`.
        let text = |k: &str| message[k].as_str().unwrap_or_default().trim();
        let rationale = Some(text("content"))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| text("reasoning"))
            .to_string();
        let tool_calls = message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        self.messages.push(message);
        self.needs_repair = false;

        if tool_calls.is_empty() || finish_reason != "tool_calls" {
            self.narrative = Some(rationale).filter(|r| !r.is_empty());
            return Ok(Vec::new());
        }
        let rationale = if rationale.is_empty() {
            "(model gave no rationale text)".to_string()
        } else {
            rationale
        };
        let mut calls = Vec::new();
        for tc in &tool_calls {
            let id = tc
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("tool call missing 'id': {tc}"))?;
            let name = tc
                .pointer("/function/name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("tool call missing 'function.name': {tc}"))?;
            let raw = tc.pointer("/function/arguments").and_then(Value::as_str);
            let raw = raw.unwrap_or("{}");
            match parse_arguments(raw) {
                Ok(arguments) => calls.push(PlannedCall {
                    id: Some(id.to_string()),
                    tool: name.to_string(),
                    arguments,
                    rationale: rationale.clone(),
                }),
                Err(e) => self.push_tool_message(id, format!("error: {e}")),
            }
        }
        self.needs_repair = calls.is_empty();
        Ok(calls)
    }

    /// Errors surface in logs and the dossier label: scrub the API key defensively.
    fn redact(&self, error: String) -> String {
        if self.config.api_key.is_empty() {
            return error;
        }
        error.replace(&self.config.api_key, "[redacted]")
    }

    async fn send(&self, body: &Value) -> Result<Value, String> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let mut retries = 0;
        let response = loop {
            let response = self
                .config
                .http
                .post(&url)
                .bearer_auth(&self.config.api_key)
                .json(body)
                .send()
                .await
                .map_err(|e| format!("LLM API request failed: {}", e.without_url()))?;
            if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS
                || retries == MAX_RATE_LIMIT_RETRIES
            {
                break response;
            }
            retries += 1;
            let wait = retry_after_secs(response.headers()).min(MAX_RETRY_WAIT_SECS);
            warn!("LLM API rate limited (HTTP 429); retry {retries} in {wait}s");
            tokio::time::sleep(Duration::from_secs(wait)).await;
        };
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("LLM API body read failed: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "LLM API returned HTTP {}: {}",
                status.as_u16(),
                text.chars().take(500).collect::<String>()
            ));
        }
        serde_json::from_str(&text).map_err(|e| format!("LLM API returned invalid JSON: {e}"))
    }
}

/// Seconds from a `retry-after` header (default 5 when absent or unparseable).
fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> u64 {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(5, |s| s.ceil().max(1.0) as u64)
}

/// `function.arguments` is a JSON *string* that must decode to an object.
fn parse_arguments(raw: &str) -> Result<Value, String> {
    let raw = if raw.trim().is_empty() { "{}" } else { raw };
    match serde_json::from_str::<Value>(raw) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(v) => Err(format!("tool arguments must be a JSON object, got: {v}")),
        Err(e) => Err(format!("tool arguments are not valid JSON ({e}): {raw}")),
    }
}

impl Planner for OpenAiPlanner {
    async fn plan(
        &mut self,
        claim_id: &str,
        history: &[ToolOutcome],
    ) -> Result<Vec<PlannedCall>, String> {
        self.absorb(claim_id, history);
        for _ in 0..=MAX_REPAIRS {
            let body = self.request_body();
            let response = self.send(&body).await.map_err(|e| self.redact(e))?;
            let calls = self.apply_response(response).map_err(|e| self.redact(e))?;
            if !self.needs_repair {
                return Ok(calls);
            }
        }
        Err(format!(
            "model sent unparseable tool arguments {} times in a row",
            MAX_REPAIRS + 1
        ))
    }

    fn narrative(&self) -> Option<&str> {
        self.narrative.as_deref()
    }
}

#[cfg(test)]
#[path = "openai_planner_tests.rs"]
mod tests;
