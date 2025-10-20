use std::fmt;
use thiserror::Error;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use vaultless_core::VaultlessError;

/// API error response wrapper
#[derive(Debug, Error)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub error_code: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            error_code: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, message)
    }

    pub fn internal_server_error(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = if let Some(code) = self.error_code {
            json!({
                "error": {
                    "message": self.message,
                    "code": code,
                    "status": self.status.as_u16(),
                }
            })
        } else {
            json!({
                "error": {
                    "message": self.message,
                    "status": self.status.as_u16(),
                }
            })
        };

        (self.status, Json(body)).into_response()
    }
}

// <-- ADDED: Implement Display for use with {} -->
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // This formats the error for logging/display purposes.
        // It includes the status code and the primary message.
        write!(f, "[HTTP {}] {}", self.status.as_u16(), self.message)
    }
}

/// Convert VaultlessError to ApiError
impl From<VaultlessError> for ApiError {
    fn from(err: VaultlessError) -> Self {
        let status =
            StatusCode::from_u16(err.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let message = if err.is_client_error() {
            err.to_string()
        } else {
            // Don't expose internal errors to clients
            tracing::error!("Internal error: {:?}", err);
            "An internal error occurred".to_string()
        };

        Self::new(status, message)
    }
}

/// Convert anyhow::Error to ApiError
impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        tracing::error!("Anyhow error: {:?}", err);
        Self::internal_server_error("An unexpected error occurred")
    }
}

/// Convert sqlx::Error to ApiError
impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!("Database error: {:?}", err);

        match err {
            sqlx::Error::RowNotFound => Self::not_found("Resource not found"),
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                Self::conflict("Resource already exists")
            }
            _ => Self::internal_server_error("Database error occurred"),
        }
    }
}

