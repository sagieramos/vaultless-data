use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};

use crate::{
    middleware::error::ApiError,
    services::token::{SessionData, TokenService},
    state::AppState,
};

/// Extract Bearer token from Authorization header
fn extract_bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let auth_header = headers
        .get("Authorization")
        .ok_or_else(|| ApiError::unauthorized("Missing Authorization header"))?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid Authorization header"))?;

    // Must be "Bearer <token>"
    if !auth_str.starts_with("Bearer ") {
        return Err(ApiError::unauthorized(
            "Invalid Authorization format. Expected: Bearer <token>",
        ));
    }

    let token = auth_str.trim_start_matches("Bearer ").trim();

    if token.is_empty() {
        return Err(ApiError::unauthorized("Empty bearer token"));
    }

    Ok(token.to_string())
}

/// Middleware to require token-based authentication (for user endpoints)
pub async fn require_token_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Extract bearer token
    let token = extract_bearer_token(request.headers())?;

    // Verify token and get session
    let token_service = TokenService::new(state.db.clone(), state.cache.clone());
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
