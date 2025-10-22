//vaultless-api/src/middleware/api_key_auth.rs
use axum::{
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use vaultless_core::ApiKey;

use crate::config::AuthHeader;

use crate::{
    middleware::error::ApiError,
    services::cache::{CacheService, api_key_cache_key},
    state::AppState,
};

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

/// Validate API key with cache-first lookup
pub async fn validate_api_key(state: &AppState, api_key: &str) -> Result<ApiKey, ApiError> {
    // Hash the API key
    let key_hash = vaultless_core::crypto::hash_content(api_key.as_bytes());

    tracing::debug!("Validating API key. Hash: {}", key_hash);

    // Try cache first
    let cache = CacheService::new(state.cache.clone(), state.config.cache.default_ttl);
    let cache_key = api_key_cache_key(&key_hash);

    // Check cache
    if let Ok(Some(cached_key)) = cache.get::<ApiKey>(&cache_key).await {
        tracing::debug!("API key cache hit");

        // Validate cached key
        cached_key.validate().map_err(|e| match e {
            vaultless_core::VaultlessError::ApiKeyInactive => {
                ApiError::forbidden("API key is inactive")
            }
            vaultless_core::VaultlessError::ApiKeyExpired => {
                ApiError::unauthorized("API key has expired")
            }
            _ => ApiError::from(e),
        })?;

        return Ok(cached_key);
    }

    // Cache miss - look up in database
    tracing::debug!("API key cache miss, checking database");

    let api_key_record = ApiKey::find_by_hash(&state.db, &key_hash)
        .await
        .map_err(|e| match e {
            vaultless_core::VaultlessError::NotFound(_) => {
                ApiError::unauthorized("Invalid API key")
            }
            _ => ApiError::from(e),
        })?;

    // Validate key is usable
    api_key_record.validate().map_err(|e| match e {
        vaultless_core::VaultlessError::ApiKeyInactive => {
            ApiError::forbidden("API key is inactive")
        }
        vaultless_core::VaultlessError::ApiKeyExpired => {
            ApiError::unauthorized("API key has expired")
        }
        _ => ApiError::from(e),
    })?;

    // Cache the API key (cache for 5 minutes)
    let cache_ttl = std::time::Duration::from_secs(300);
    if let Err(e) = cache
        .set_with_ttl(&cache_key, &api_key_record, cache_ttl)
        .await
    {
        tracing::warn!("Failed to cache API key: {}", e);
        // Don't fail the request
    } else {
        tracing::debug!("API key cached");
    }

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

    // Validate API key (with caching)
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
