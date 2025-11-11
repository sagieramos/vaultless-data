// vaultless-api/src/middleware/token_auth.rs
use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};

use uuid::Uuid;

use crate::{
    middleware::error::ApiError,
    services::token::{SessionData, TokenService},
    state::AppState,
};

use crate::config::AuthHeader;

use axum::{
    extract::{FromRequestParts},
    http::request::Parts,
};

use std::sync::Arc;


/// Extract Bearer token from Authorization header
fn extract_bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let auth_header = headers
        .get("Authorization")
        .ok_or_else(|| ApiError::unauthorized("Missing Authorization header"))?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid Authorization header"))?;

    // Must be "Bearer <token>"
    if !auth_str.starts_with(AuthHeader::BEARER) {
        return Err(ApiError::unauthorized(
            "Invalid Authorization format. Expected: Bearer <token>",
        ));
    }

    let token = auth_str.trim_start_matches(AuthHeader::BEARER).trim();

    if token.is_empty() {
        return Err(ApiError::unauthorized("Empty bearer token"));
    }

    Ok(token.to_string())
}

/// Middleware to require token-based authentication (for user endpoints)
pub async fn require_user_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Extract bearer token
    let token = extract_bearer_token(request.headers())?;

    // Verify token and get session
    let token_service = TokenService::new(state.db, state.redis_pool);
    let session_data = token_service.verify_access_token(&token).await?;

    // Store session data in request extensions
    request.extensions_mut().insert(session_data);

    // Continue to handler
    Ok(next.run(request).await)
}

/// Extractor for SessionData in handlers
/// Usage: `session: SessionData` in handler parameters
impl<S> axum::extract::FromRequestParts<S> for SessionData
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<SessionData>()
            .cloned()
            .ok_or_else(|| ApiError::unauthorized("Missing session data"))
    }
}

/*
GET /v1/api/keys/details
Authorization: Bearer <user_access_token>
X-Api-Key-Id: 1f4b933a-8e1b-4c3a-bca0-0b179d0e8a61

 */
/// Middleware to ensure the authenticated user owns the API key specified in the header
pub async fn require_api_key_ownership(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Require session first (user must be authenticated)
    let session = request
        .extensions()
        .get::<SessionData>()
        .cloned()
        .ok_or_else(|| ApiError::unauthorized("You must be logged in to access this resource"))?;

    // Extract API key ID from header
    let key_id = extract_key_id_from_header(request.headers())?;

    // Check ownership
    let query = r#"
        SELECT 1 FROM api_keys WHERE id = $1 AND user_id = $2 LIMIT 1
    "#;

    let owned = sqlx::query(query)
        .bind(key_id)
        .bind(session.user_id)
        .fetch_optional(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!("DB error during ownership check: {}", e);
            ApiError::forbidden("You do not own this API key")
        })?;

    if owned.is_none() {
        return Err(ApiError::forbidden("You do not own this API key"));
    }

    // Store key_id for downstream use
    request.extensions_mut().insert(key_id);

    Ok(next.run(request).await)
}

/// Extract and validate `X-Api-Key-Id` header
fn extract_key_id_from_header(headers: &HeaderMap) -> Result<Uuid, ApiError> {
    let value = headers
        .get(AuthHeader::API_KEY)
        .ok_or_else(|| ApiError::bad_request("Missing X-Api-Key-Id header"))?
        .to_str()
        .map_err(|_| ApiError::bad_request("Invalid X-Api-Key-Id header"))?;

    Uuid::parse_str(value)
        .map_err(|_| ApiError::bad_request("Invalid UUID format in X-Api-Key-Id header"))
}
