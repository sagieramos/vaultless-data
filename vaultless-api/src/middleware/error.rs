use std::fmt;
use thiserror::Error;

use axum::{
    http::{header, StatusCode},
    Json,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tracing::error;
use vaultless_core::VaultlessError;

/// API error response wrapper
#[derive(Debug, Error)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub error_code: Option<String>,
    #[cfg(feature = "tracing")] // Optional: For trace ID
    pub trace_id: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            error_code: None,
            #[cfg(feature = "tracing")]
            trace_id: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }

    #[cfg(feature = "tracing")]
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
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
        let mut response = if let Some(ref code) = self.error_code {
            let error_json = json!({
                "error": {
                    "message": self.message,
                    "code": code,
                    "status": self.status.as_u16(),
                }
            });
            (self.status, Json(error_json)).into_response()
        } else {
            let error_json = json!({
                "error": {
                    "message": self.message,
                    "status": self.status.as_u16(),
                }
            });
            (self.status, Json(error_json)).into_response()
        };

        // Add error code header if present
        if let Some(code) = self.error_code {
            if let Ok(header_value) = code.parse() {
                response.headers_mut().insert(header::WARNING, header_value);
            }
        }

        #[cfg(feature = "tracing")]
        if let Some(trace_id) = self.trace_id {
            if let Ok(header_value) = trace_id.parse() {
                response.headers_mut().insert("X-Trace-ID", header_value);
            }
        }

        response
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[HTTP {}] {}", self.status.as_u16(), self.message)
    }
}

/// Convert VaultlessError to ApiError
impl From<VaultlessError> for ApiError {
    fn from(err: VaultlessError) -> Self {
        match err {
            // --- Authentication / authorization errors ---
            VaultlessError::EmailNotVerified(_) => ApiError::unauthorized(
                "Email not verified. A new verification email has been sent.",
            )
            .with_code("EMAIL_NOT_VERIFIED"),

            VaultlessError::Unauthorized(msg) => {
                ApiError::unauthorized(msg).with_code("UNAUTHORIZED")
            }

            VaultlessError::Forbidden(msg) => ApiError::forbidden(msg).with_code("FORBIDDEN"),

            VaultlessError::InvalidApiKey => {
                ApiError::unauthorized("Invalid API key").with_code("INVALID_API_KEY")
            }

            VaultlessError::ApiKeyExpired => {
                ApiError::unauthorized("API key expired").with_code("API_KEY_EXPIRED")
            }

            VaultlessError::ApiKeyInactive => {
                ApiError::forbidden("API key inactive").with_code("API_KEY_INACTIVE")
            }

            // --- Client-side errors ---
            VaultlessError::NotFound(msg) => ApiError::not_found(msg).with_code("NOT_FOUND"),

            VaultlessError::Validation(msg) | VaultlessError::BadRequest(msg) => {
                ApiError::bad_request(msg).with_code("BAD_REQUEST")
            }

            VaultlessError::Duplicate(msg) | VaultlessError::Conflict(msg) => {
                ApiError::conflict(msg).with_code("CONFLICT")
            }

            // --- Rate limit / quota errors ---
            VaultlessError::QuotaExceeded(msg) => {
                ApiError::too_many_requests(msg).with_code("QUOTA_EXCEEDED")
            }

            VaultlessError::RateLimitExceeded => {
                ApiError::too_many_requests("Rate limit exceeded").with_code("RATE_LIMIT_EXCEEDED")
            }

            // --- Message lifecycle ---
            VaultlessError::MessageExpired => {
                ApiError::new(StatusCode::GONE, "Message expired").with_code("MESSAGE_EXPIRED")
            }

            VaultlessError::MessageAccessLimitReached => {
                ApiError::new(StatusCode::GONE, "Message access limit reached")
                    .with_code("MESSAGE_ACCESS_LIMIT_REACHED")
            }

            // --- Internal errors (do not expose message) ---
            _ => {
                error!("Internal error: {}", err.to_string()); // Sanitized
                ApiError::internal_server_error("An internal error occurred")
                    .with_code("INTERNAL_ERROR")
            }
        }
    }
}

/// Convert anyhow::Error to ApiError
impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        error!("Anyhow error: {}", err.to_string()); // Sanitized
        Self::internal_server_error("An unexpected error occurred")
    }
}

/// Convert sqlx::Error to ApiError
impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        error!("Database error: {}", err.to_string()); // Sanitized

        match err {
            sqlx::Error::RowNotFound => Self::not_found("Resource not found"),
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                Self::conflict("Resource already exists")
            }
            sqlx::Error::Database(db_err) if db_err.is_foreign_key_violation() => {
                Self::bad_request("Invalid reference to related resource")
            }
            sqlx::Error::Database(_) => Self::internal_server_error("Database error occurred"),
            _ => Self::internal_server_error("Database error occurred"),
        }
    }
}

// Catch-all for other errors
impl From<Box<dyn std::error::Error + Send + Sync>> for ApiError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        error!("Unexpected error: {}", err.to_string());
        Self::internal_server_error("An unexpected error occurred")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vaultless_core::VaultlessError;

    #[test]
    fn test_from_vaultless_error_auth() {
        let err = VaultlessError::ApiKeyInactive;
        let api_err: ApiError = err.into();
        assert_eq!(api_err.status, StatusCode::FORBIDDEN);
        assert_eq!(api_err.error_code, Some("API_KEY_INACTIVE".to_string()));
        assert_eq!(api_err.message, "API key inactive");
    }

    #[test]
    fn test_from_vaultless_error_internal() {
        let err = VaultlessError::Internal("test".into());
        let api_err: ApiError = err.into();
        assert_eq!(api_err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(api_err.error_code, Some("INTERNAL_ERROR".to_string()));
        assert_eq!(api_err.message, "An internal error occurred");
    }

    #[test]
    fn test_from_sqlx_row_not_found() {
        let err = sqlx::Error::RowNotFound;
        let api_err: ApiError = err.into();
        assert_eq!(api_err.status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_from_anyhow() {
        let err = anyhow::anyhow!("test error");
        let api_err: ApiError = err.into();
        assert_eq!(api_err.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_into_response() {
        let err = ApiError::unauthorized("Test").with_code("TEST_CODE");
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        // Check headers if feature enabled
    }
}