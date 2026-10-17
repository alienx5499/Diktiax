//! Router wiring: paths -> handlers, CORS and request tracing.

use super::handlers;
use super::rate_limit::Cooldown;
use crate::agent::ClaimInvestigator;
use crate::db::FalkorGraphRepository;
use axum::routing::{get, post};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub struct AppState {
    pub repo: FalkorGraphRepository,
    pub investigator: ClaimInvestigator,
    /// Per-visitor cooldown on `POST /api/investigate/{id}` (the only LLM-backed route).
    pub investigate_cooldown: Cooldown,
}

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    Router::new()
        .route("/api/health", get(handlers::health))
        .route(
            "/api/claims",
            get(handlers::list_claims).post(handlers::create_claim),
        )
        .route("/api/claims/{id}", get(handlers::get_claim))
        .route(
            "/api/claims/{id}/investigations",
            get(handlers::investigation_history),
        )
        .route("/api/investigate/{id}", post(handlers::investigate))
        .route("/api/graph/{id}", get(handlers::graph))
        .route("/api/seed", post(handlers::seed))
        .route("/api/mcp/tools", get(handlers::mcp_tools))
        .route("/api/mcp/call", post(handlers::mcp_call))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state))
}
