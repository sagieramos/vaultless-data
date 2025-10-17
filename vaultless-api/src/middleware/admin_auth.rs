use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use vaultless_core::ApiKey;

use crate::{middleware::error::ApiError, state::AppState};

/// Admin authentication middleware
pub async fn require_admin(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Extract admin API key from X-Admin-Key header
    let admin_key = headers
        .get("X-Admin-Key")
        .ok_or_else(|| ApiError::unauthorized("Missing X-Admin-Key header"))?
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid X-Admin-Key header"))?;

    // Hash and verify admin key
    let key_hash = vaultless_core::hash_content(admin_key.as_bytes());

    // Check against admin key from environment
    let expected_hash =
        vaultless_core::hash_content(state.config.security.admin_api_key.as_bytes());

    if key_hash != expected_hash {
        tracing::warn!("Invalid admin key attempt");
        return Err(ApiError::forbidden("Invalid admin credentials"));
    }

    // Continue to handler
    Ok(next.run(request).await)
}

/// Middleware to require API key to be in admin role (future: RBAC)
pub async fn require_admin_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // For now, just check if API key exists and is valid
    // Future: Add roles table and check for admin role

    let auth_header = headers
        .get("Authorization")
        .ok_or_else(|| ApiError::unauthorized("Missing Authorization header"))?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid Authorization header"))?;

    let api_key = if auth_str.starts_with("Bearer ") {
        auth_str.trim_start_matches("Bearer ")
    } else {
        auth_str
    };

    let key_hash = vaultless_core::hash_content(api_key.as_bytes());
    let api_key_record = ApiKey::find_by_hash(&state.db, &key_hash)
        .await
        .map_err(|_| ApiError::forbidden("Invalid admin API key"))?;

    api_key_record
        .validate()
        .map_err(|_| ApiError::forbidden("Admin API key is not active"))?;

    // TODO: Check if key has admin role in database
    // For now, check if organization field contains "admin"
    let is_admin = api_key_record
        .organization
        .as_ref()
        .map(|org| org.to_lowercase().contains("admin"))
        .unwrap_or(false);

    if !is_admin {
        return Err(ApiError::forbidden("Requires admin privileges"));
    }

    request.extensions_mut().insert(api_key_record);
    Ok(next.run(request).await)
}
