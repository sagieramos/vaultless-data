// vaultless-api/src/middleware/api_key_auth.rs
use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use vaultless_core::{ApiKey, crypto};

use crate::config::AuthHeader;

use crate::{middleware::error::ApiError, state::AppState};

/// Extract API key from request headers
/// Supports both "Authorization: X-api-key-id <key>" and "Authorization: <key>"
pub async fn extract_api_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let auth_header = headers
        .get("Authorization")
        .ok_or_else(|| ApiError::unauthorized("Missing Authorization header"))?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid Authorization header"))?;

    // Support both "X-api-key-id token" and raw token
    let api_key = if auth_str.starts_with(AuthHeader::API_KEY) {
        auth_str.trim_start_matches(AuthHeader::API_KEY).trim()
    } else {
        auth_str
    };

    if api_key.is_empty() {
        return Err(ApiError::unauthorized("Empty API key"));
    }

    Ok(api_key.to_string())
}

/// Validate API key (relies on core's internal caching via find_by_hash)
pub async fn validate_api_key(state: &AppState, api_key: &str) -> Result<ApiKey, ApiError> {
    tracing::debug!("Validating API key: {}", api_key);

    let key_hash = crypto::hash_content(api_key.as_bytes());

    // --- FIX: Clone the owned DB pool value ---
    let db_pool = state.db.clone();

    // The Redis pool is already being handled correctly with an Arc wrapper.

    // Look up in database (with core-level Redis caching)
    let api_key_record = ApiKey::find_by_hash(&db_pool, Some(state.redis_pool.clone()), key_hash)
        .await
        .map_err(ApiError::from)?;

    // Validate key is usable (e.g., active, not expired)
    api_key_record
        .validate(db_pool.as_ref(), Some(state.redis_pool.clone()))
        .await
        .map_err(ApiError::from)?;

    tracing::debug!("API key validated successfully");
    Ok(api_key_record)
}

/// Middleware to require API key authentication
pub async fn require_client_api_key(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Extract API key from headers
    let api_key_str = extract_api_key(request.headers()).await?;

    // Validate API key (with core caching)
    let api_key = validate_api_key(&state, &api_key_str).await?;

    tracing::debug!(
        api_key_id = %api_key.id,
        tier = ?api_key.tier,
        "API key authenticated"
    );

    // Store API key in request extensions for handlers to use
    request.extensions_mut().insert(api_key);

    // Continue to handler
    Ok(next.run(request).await)
}

/// Extractor for ApiKey in handlers
/// Usage: `AuthenticatedApiKey(api_key): AuthenticatedApiKey` in handler parameters
/// This is a local newtype wrapper around the foreign `ApiKey` so we can implement
/// the foreign `FromRequestParts` trait (satisfies Rust's orphan rules).
#[derive(Clone)]
pub struct AuthenticatedApiKey(pub ApiKey);

impl<S> axum::extract::FromRequestParts<S> for AuthenticatedApiKey
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
            .get::<ApiKey>()
            .cloned()
            .map(AuthenticatedApiKey)
            .ok_or_else(|| ApiError::unauthorized("Missing API key"))
    }
}

impl std::ops::Deref for AuthenticatedApiKey {
    type Target = ApiKey;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[tokio::test]
    async fn test_extract_api_key_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("X-Api-Key-Id vlt_test_key_123"),
        );

        let result = extract_api_key(&headers).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "vlt_test_key_123");
    }

    #[tokio::test]
    async fn test_extract_api_key_raw() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("vlt_test_key_456"),
        );

        let result = extract_api_key(&headers).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "vlt_test_key_456");
    }

    #[tokio::test]
    async fn test_extract_api_key_missing() {
        let headers = HeaderMap::new();
        let result = extract_api_key(&headers).await;
        assert!(result.is_err());
    }
}
