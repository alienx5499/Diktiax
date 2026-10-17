//! HTTP handlers. Validation errors are 400, unknown ids 404, FalkorDB failures 502.

use super::error::ApiError;
use super::routes::AppState;
use crate::agent::InvestigationDossier;
use crate::domain::{ClaimSummary, InvestigationRecord, NewClaim, SubgraphPayload};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type Shared = State<Arc<AppState>>;
type ApiResult<T> = Result<Json<T>, ApiError>;

pub async fn health(State(state): Shared) -> ApiResult<Value> {
    state.repo.ping().await.map_err(ApiError::Upstream)?;
    Ok(Json(json!({
        "status": "ok",
        "falkordb": "ok",
        "graph_name": state.repo.graph_name(),
        "planner": state.investigator.planner_label(),
        "seed_enabled": seed_allowed(admin_token().as_deref(), None, dev_mode()),
    })))
}

pub async fn list_claims(State(state): Shared) -> ApiResult<Vec<ClaimSummary>> {
    state
        .repo
        .list_claims()
        .await
        .map(Json)
        .map_err(ApiError::Upstream)
}

pub async fn create_claim(
    State(state): Shared,
    body: Result<Json<NewClaim>, JsonRejection>,
) -> Result<(StatusCode, Json<ClaimSummary>), ApiError> {
    let Json(claim) = body?;
    claim.validate().map_err(ApiError::BadRequest)?;
    let created = state
        .repo
        .ingest_claim(claim)
        .await
        .map_err(ApiError::from_upstream)?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub async fn get_claim(State(state): Shared, Path(id): Path<String>) -> ApiResult<ClaimSummary> {
    match state
        .repo
        .get_claim(&id)
        .await
        .map_err(ApiError::Upstream)?
    {
        Some(claim) => Ok(Json(claim)),
        None => Err(ApiError::NotFound(format!("claim {id} not found"))),
    }
}

pub async fn investigation_history(
    State(state): Shared,
    Path(id): Path<String>,
) -> ApiResult<Vec<InvestigationRecord>> {
    state
        .repo
        .investigation_history(&id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn investigate(
    State(state): Shared,
    Path(id): Path<String>,
) -> ApiResult<InvestigationDossier> {
    state
        .investigator
        .investigate(&id)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

pub async fn graph(State(state): Shared, Path(id): Path<String>) -> ApiResult<SubgraphPayload> {
    state
        .repo
        .claim_subgraph(&id, 3)
        .await
        .map(Json)
        .map_err(ApiError::from_upstream)
}

/// `POST /api/seed` wipes the graph, so it fails closed: it needs a matching `x-admin-token`
/// header when `ADMIN_TOKEN` is set, and is refused when it isn't, unless `DIKTIAX_DEV=1`
/// explicitly opts a local run in. The `--seed` CLI flag is unaffected.
pub async fn seed(State(state): Shared, headers: HeaderMap) -> ApiResult<Value> {
    let provided = headers.get("x-admin-token").and_then(|v| v.to_str().ok());
    if !seed_allowed(admin_token().as_deref(), provided, dev_mode()) {
        return Err(ApiError::Forbidden(
            "resetting the graph requires a valid x-admin-token (or DIKTIAX_DEV=1 locally)".into(),
        ));
    }
    let report = state.repo.seed(true).await.map_err(ApiError::Upstream)?;
    Ok(Json(json!({
        "status": "SUCCESS",
        "message": format!("Seeded {} nodes, {} edges", report.nodes, report.edges),
        "report": report,
    })))
}

pub async fn mcp_tools() -> Json<Value> {
    Json(json!({ "tools": crate::tools::external_tool_definitions() }))
}

#[derive(Deserialize)]
pub struct McpCall {
    name: String,
    #[serde(default)]
    arguments: Value,
}

pub async fn mcp_call(
    State(state): Shared,
    body: Result<Json<McpCall>, JsonRejection>,
) -> ApiResult<Value> {
    let Json(call) = body?;
    if !crate::tools::external_tool_definitions()
        .iter()
        .any(|t| t.name == call.name)
    {
        return Err(ApiError::BadRequest(format!(
            "unknown tool '{}'",
            call.name
        )));
    }
    match crate::tools::execute_external_tool(&state.repo, &call.name, &call.arguments).await {
        Ok(result) => Ok(Json(json!({ "tool": call.name, "result": result }))),
        // mcp argument validation errors all name the offending argument.
        Err(e) if e.contains("argument") => Err(ApiError::BadRequest(e)),
        Err(e) => Err(ApiError::from_upstream(e)),
    }
}

fn admin_token() -> Option<String> {
    std::env::var("ADMIN_TOKEN").ok().filter(|t| !t.is_empty())
}

fn dev_mode() -> bool {
    std::env::var("DIKTIAX_DEV").is_ok_and(|v| v == "1")
}

/// Single seed policy shared by `seed` and `health` (which advertises it as `seed_enabled`).
fn seed_allowed(expected: Option<&str>, provided: Option<&str>, dev: bool) -> bool {
    match expected {
        None => dev,
        Some(token) => provided == Some(token),
    }
}

#[cfg(test)]
mod tests {
    use super::seed_allowed;

    #[test]
    fn seed_fails_closed() {
        assert!(
            !seed_allowed(None, None, false),
            "no token configured and no dev opt-in"
        );
        assert!(seed_allowed(None, None, true));
        assert!(seed_allowed(Some("s3cret"), Some("s3cret"), false));
        assert!(
            !seed_allowed(Some("s3cret"), None, true),
            "a configured token wins over dev mode"
        );
        assert!(!seed_allowed(Some("s3cret"), Some("wrong"), false));
    }
}
