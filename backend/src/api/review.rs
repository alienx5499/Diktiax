//! `POST /api/claims/{id}/review`: the assigned investigator confirms fraud or releases a frozen
//! payout. The reviewer id in the body is asserted by a trusted operator, not authenticated per
//! person, so the route needs the admin token (`x-admin-token`, same policy as `/api/seed`;
//! open locally with `DIKTIAX_DEV=1`). In a real deployment it sits behind SSO. Rate-limited per
//! visitor.

use super::error::ApiError;
use super::handlers::{admin_token, dev_mode, seed_allowed};
use super::rate_limit::visitor_key;
use super::routes::AppState;
use crate::domain::{ReviewError, ReviewRecord, ReviewRequest};
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::{Extension, Json};
use std::net::SocketAddr;
use std::sync::Arc;

/// One review per visitor per this many seconds.
pub const REVIEW_COOLDOWN_SECS: u64 = 5;

impl From<ReviewError> for ApiError {
    fn from(error: ReviewError) -> Self {
        match error {
            ReviewError::NotFound(m) => Self::NotFound(m),
            ReviewError::Forbidden(m) => Self::Forbidden(m),
            ReviewError::Conflict(m) => Self::Conflict(m),
            ReviewError::Upstream(m) => Self::Upstream(m),
        }
    }
}

/// 429 with `Retry-After` while `visitor` is inside the review cooldown.
fn check_review_cooldown(state: &AppState, visitor: &str) -> Result<(), ApiError> {
    state
        .review_cooldown
        .check(visitor)
        .map_err(|wait| ApiError::RateLimited {
            message: format!(
                "One review every {} s per visitor. Try again in {wait} s.",
                state.review_cooldown.period_secs()
            ),
            retry_after_secs: wait,
        })
}

pub async fn review_claim(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    body: Result<Json<ReviewRequest>, JsonRejection>,
) -> Result<Json<ReviewRecord>, ApiError> {
    let provided = headers.get("x-admin-token").and_then(|v| v.to_str().ok());
    if !seed_allowed(admin_token().as_deref(), provided, dev_mode()) {
        return Err(ApiError::Forbidden(
            "reviewing a payout requires a valid x-admin-token (the reviewer id is asserted by a \
             trusted operator; DIKTIAX_DEV=1 opens it locally)"
                .into(),
        ));
    }
    let Json(request) = body?;
    let request = request.normalized().map_err(ApiError::BadRequest)?;
    let visitor = visitor_key(&headers, peer.map(|Extension(ConnectInfo(addr))| addr));
    check_review_cooldown(&state, &visitor)?;
    Ok(Json(state.repo.review_claim(&id, &request).await?))
}
