use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use vaultless_core::{ApiKey, VaultlessError};

use crate::{middleware::error::ApiError, state::AppState};

/// Extract and validate API key from request headers
pub async fn extract_api_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let auth_header = headers
        .get("Authorization")
        .ok_or_else(|| ApiError::unauthorized("Missing Authorization header"))?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| ApiError::unauthorized("Invalid Authorization header"))?;

    // Support both "Bearer token" and raw token
    let api_key = if auth_str.starts_with("Bearer ") {
        auth_str.trim_start_matches("Bearer ")
    } else {
        auth_str
    };

    if api_key.is_empty() {
        return Err(ApiError::unauthorized("Empty API key"));
    }

    Ok(api_key.to_string())
}

/// Validate API key exists and is active
pub async fn validate_api_key(
    state: &AppState,
    api_key: &str,
) -> Result<ApiKey, ApiError> {
    // Hash the API key
    let key_hash = vaultless_core::hash_content(api_key.as_bytes());

    // Look up in database
    let api_key_record = ApiKey::find_by_hash(&state.db, &key_hash)
        .await
        .map_err(|e| match e {
            VaultlessError::NotFound(_) => ApiError::unauthorized("Invalid API key"),
            _ => ApiError::from(e),
        })?;

    // Validate key is usable
    api_key_record.validate().map_err(|e| match e {
        VaultlessError::ApiKeyInactive => {
            ApiError::forbidden("API key is inactive")
        }
        VaultlessError::ApiKeyExpired => {
            ApiError::unauthorized("API key has expired")
        }
        _ => ApiError::from(e),
    })?;

    Ok(api_key_record)
}

/// Middleware to require authentication
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Extract API key from headers
    let api_key_str = extract_api_key(request.headers()).await?;

    // Validate API key
    let api_key = validate_api_key(&state, &api_key_str).await?;

    // Store API key in request extensions for handlers to use
    request.extensions_mut().insert(api_key);

    // Continue to handler
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_extract_api_key_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_static("Bearer test_key_123"));

        match tokio_test::block_on(extract_api_key(&headers)) {
            Ok(s) => assert_eq!(s, "test_key_123"),
            Err(_) => panic!("expected Ok"),
        }
    }

    #[test]
    fn test_extract_api_key_raw() {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_static("test_key_456"));

        match tokio_test::block_on(extract_api_key(&headers)) {
            Ok(s) => assert_eq!(s, "test_key_456"),
            Err(_) => panic!("expected Ok"),
        }
    }

    #[test]
    fn test_extract_api_key_missing() {
        let headers = HeaderMap::new();
        let result = tokio_test::block_on(extract_api_key(&headers));
        assert!(result.is_err());
    }
}