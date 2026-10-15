//! MCP stdio transport: newline-delimited JSON-RPC 2.0 on stdin/stdout.
//! stdout carries protocol lines only; diagnostics go through `tracing` (stderr).

use super::catalog::INVESTIGATE_CLAIM;
use super::protocol::{route_line, tool_result, Route};
use super::tools::{execute_tool, required_str};
use crate::agent::ClaimInvestigator;
use crate::db::FalkorGraphRepository;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn};

/// Everything a tool call may need.
pub struct McpContext {
    pub repo: FalkorGraphRepository,
    pub investigator: ClaimInvestigator,
}

impl McpContext {
    async fn call_tool(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        if name == INVESTIGATE_CLAIM {
            let claim_id = required_str(arguments, "claim_id")?;
            let dossier = self.investigator.investigate(&claim_id).await?;
            return serde_json::to_value(dossier)
                .map_err(|e| format!("failed to serialize dossier: {e}"));
        }
        execute_tool(&self.repo, name, arguments).await
    }
}

/// Maps one inbound line to the serialized response line, if any.
pub async fn handle_message(ctx: &McpContext, line: &str) -> Option<String> {
    let response = match route_line(line) {
        Route::Silent => return None,
        Route::Respond(response) => response,
        Route::CallTool {
            id,
            name,
            arguments,
        } => {
            let outcome = ctx.call_tool(&name, &arguments).await;
            if let Err(e) = &outcome {
                warn!("MCP tool '{name}' failed: {e}");
            }
            tool_result(id, outcome)
        }
    };
    Some(response.to_string())
}

/// Serves MCP over stdio until stdin closes. Requests are handled sequentially.
pub async fn serve_stdio(
    repo: FalkorGraphRepository,
    investigator: ClaimInvestigator,
) -> Result<(), String> {
    let ctx = McpContext { repo, investigator };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    info!("Diktiax MCP server listening on stdio");
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("failed to read stdin: {e}"))?
    {
        if line.trim().is_empty() {
            continue;
        }
        let Some(mut response) = handle_message(&ctx, &line).await else {
            continue;
        };
        response.push('\n');
        stdout
            .write_all(response.as_bytes())
            .await
            .map_err(|e| format!("failed to write stdout: {e}"))?;
        stdout
            .flush()
            .await
            .map_err(|e| format!("failed to flush stdout: {e}"))?;
    }
    info!("stdin closed; MCP server shutting down");
    Ok(())
}
