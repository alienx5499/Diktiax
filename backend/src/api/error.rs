//! Single API error type mapped to HTTP status codes with a JSON `{error}` body.

use axum::extract::rejection::JsonRejection;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use tracing::error;

#[derive(Debug, PartialEq)]
pub enum ApiError {
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    /// Missing or wrong admin token on a destructive route.
    Forbidden(String),
    /// Per-visitor cooldown hit; sent as 429 with `Retry-After`.
    RateLimited {
        message: String,
        retry_after_secs: u64,
    },
    /// FalkorDB or another upstream dependency failed.
    Upstream(String),
    Internal(String),
}

impl ApiError {
    /// Classifies a repository/agent error string (the repository contract is `Result<_, String>`).
    pub fn from_upstream(message: String) -> Self {
        if message.contains("not found") {
            Self::NotFound(message)
        } else if message.contains("already exists") {
            Self::Conflict(message)
        } else {
            Self::Upstream(message)
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::BadRequest(rejection.body_text())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = match self {
            Self::RateLimited {
                message,
                retry_after_secs,
            } => {
                let headers = [(header::RETRY_AFTER, retry_after_secs.to_string())];
                return (status, headers, Json(json!({ "error": message }))).into_response();
            }
            Self::NotFound(m)
            | Self::BadRequest(m)
            | Self::Conflict(m)
            | Self::Forbidden(m)
            | Self::Upstream(m)
            | Self::Internal(m) => m,
        };
        if status.is_server_error() {
            error!("HTTP {}: {message}", status.as_u16());
        }
        (status, Json(json!({ "error": message }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_upstream_messages() {
        assert_eq!(
            ApiError::from_upstream("claim X not found".into()).status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            ApiError::from_upstream("claim X already exists".into()).status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            ApiError::from_upstream("FalkorDB query failed".into()).status(),
            StatusCode::BAD_GATEWAY
        );
    }
}
