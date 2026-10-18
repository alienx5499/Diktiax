//! HTTP handlers. Validation errors are 400, unknown ids 404, FalkorDB failures 502.

use super::error::ApiError;
use super::rate_limit::visitor_key;
use super::routes::AppState;
use crate::agent::InvestigationDossier;
use crate::domain::{ChainedVerdict, ClaimSummary, NewClaim, SubgraphPayload};
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

type Shared = State<Arc<AppState>>;
type ApiResult<T> = Result<Json<T>, ApiError>;

pub async fn health(State(state): Shared) -> ApiResult<Value> {
    state.repo.ping().await.map_err(ApiError::Upstream)?;
    Ok(Json(json!({
        "status": "ok",
        "falkordb": "ok",
        "graph_name": state.repo.graph_name(),
        "evidence_lane": state.repo.evidence_lane(),
        "planner": state.investigator.planner_label(),
        "seed_enabled": seed_allowed(admin_token().as_deref(), None, dev_mode()),
        // Same policy as seed: reviews need the admin token unless DIKTIAX_DEV=1.
        "review_enabled": seed_allowed(admin_token().as_deref(), None, dev_mode()),
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
) -> ApiResult<Vec<ChainedVerdict>> {
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
    headers: HeaderMap,
    // Optional so the router also works when served without connect info (e.g. in tests).
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> ApiResult<InvestigationDossier> {
    let visitor = visitor_key(&headers, peer.map(|Extension(ConnectInfo(addr))| addr));
    if let Err(wait) = state.investigate_cooldown.check(&visitor) {
        return Err(ApiError::RateLimited {
            message: format!(
                "One investigation every {} s per visitor. Try again in {wait} s.",
                state.investigate_cooldown.period_secs()
            ),
            retry_after_secs: wait,
        });
    }
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
    headers: HeaderMap,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Result<Json<McpCall>, JsonRejection>,
) -> ApiResult<Value> {
    let Json(call) = body?;
    ensure_http_tool(&call.name)?;
    let visitor = visitor_key(&headers, peer.map(|Extension(ConnectInfo(addr))| addr));
    // Held by the spawned query task, so a client that disconnects can't free it early.
    let mut scan_permit = None;
    if crate::tools::is_graph_scan(&call.name) {
        scan_permit = Some(scan_slot(&state.scan_slots)?);
        if let Err(wait) = state
            .scan_cooldown
            .check(&format!("{}:{visitor}", call.name))
        {
            return Err(ApiError::RateLimited {
                message: format!(
                    "One {} call every {} s per visitor. Try again in {wait} s.",
                    call.name,
                    state.scan_cooldown.period_secs()
                ),
                retry_after_secs: wait,
            });
        }
    }
    let (repo, name, args) = (
        state.repo.clone(),
        call.name.clone(),
        call.arguments.clone(),
    );
    let outcome = tokio::spawn(async move {
        let _permit = scan_permit;
        crate::tools::execute_external_tool(&repo, &name, &args).await
    })
    .await
    .map_err(|e| ApiError::from_upstream(format!("tool task failed: {e}")))?;
    match outcome {
        Ok(result) => Ok(Json(json!({ "tool": call.name, "result": result }))),
        // mcp argument validation errors all name the offending argument.
        Err(e) if e.contains("argument") => Err(ApiError::BadRequest(e)),
        Err(e) => Err(ApiError::from_upstream(e)),
    }
}

/// Only the external catalog is callable over HTTP: `investigate_claim` and `review_claim` (which
/// writes a person's decision) stay on the local stdio MCP server.
fn ensure_http_tool(name: &str) -> Result<(), ApiError> {
    if crate::tools::external_tool_definitions()
        .iter()
        .any(|t| t.name == name)
    {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!("unknown tool '{name}'")))
    }
}

/// A free graph-scan slot, or 429 with `Retry-After: 1` when every slot is busy.
fn scan_slot(slots: &Arc<Semaphore>) -> Result<OwnedSemaphorePermit, ApiError> {
    Arc::clone(slots)
        .try_acquire_owned()
        .map_err(|_| ApiError::RateLimited {
            message: "Too many graph scans running. Try again in 1 s.".into(),
            retry_after_secs: 1,
        })
}

pub(super) fn admin_token() -> Option<String> {
    std::env::var("ADMIN_TOKEN").ok().filter(|t| !t.is_empty())
}

pub(super) fn dev_mode() -> bool {
    std::env::var("DIKTIAX_DEV").is_ok_and(|v| v == "1")
}

/// Single seed policy shared by `seed` and `health` (which advertises it as `seed_enabled`).
pub(super) fn seed_allowed(expected: Option<&str>, provided: Option<&str>, dev: bool) -> bool {
    match expected {
        None => dev,
        Some(token) => provided == Some(token),
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_http_tool, scan_slot, seed_allowed, ApiError};
    use tokio::sync::Semaphore;

    #[test]
    fn scan_slots_cap_concurrent_scans() {
        let slots = std::sync::Arc::new(Semaphore::new(2));
        let first = scan_slot(&slots).expect("first slot");
        let _second = scan_slot(&slots).expect("second slot");
        match scan_slot(&slots) {
            Err(ApiError::RateLimited {
                retry_after_secs, ..
            }) => assert_eq!(retry_after_secs, 1),
            _ => panic!("a third concurrent scan must be refused with 429"),
        }
        drop(first);
        assert!(scan_slot(&slots).is_ok(), "a finished scan frees its slot");
    }

    #[test]
    fn writing_tools_are_not_callable_over_http() {
        let http = crate::tools::external_tool_definitions();
        for name in ["review_claim", "investigate_claim"] {
            assert!(
                http.iter().all(|t| t.name != name),
                "{name} listed over HTTP"
            );
            assert!(matches!(
                ensure_http_tool(name),
                Err(ApiError::BadRequest(_))
            ));
        }
        assert!(ensure_http_tool("investigation_history").is_ok());
    }

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
