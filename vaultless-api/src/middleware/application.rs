use super::helper::*;
use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{middleware::error::ApiError, state::AppState};
use vaultless_core::{ApplicationKeyView, models::applications::dto::{publishable_key_resolution_cache_key, secret_key_resolution_cache_key}};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ApplicationKeyViewExt(pub Arc<ApplicationKeyView>);

/// Extension that stores the auth cache key for hot-path operations
#[derive(Debug, Clone)]
pub struct AuthCacheKey(pub String);

pub async fn app_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let api_key = extract_api_key(req.headers())?;

    // Generate cache key BEFORE resolving (needed for v2 send_message)
    // Using dedicated functions ensures consistent key format
    let auth_cache_key = if api_key.starts_with("pk_") {
        publishable_key_resolution_cache_key(&api_key)
    } else if api_key.starts_with("sk_") {
        // For secret keys, the function handles proper hashing
        secret_key_resolution_cache_key(&api_key)
    } else {
        return Err(ApiError::unauthorized("Invalid API key format")
            .with_code("INVALID_API_KEY_FORMAT"));
    };

    let auth_config =
        ApplicationKeyView::resolve_and_validate(state.db.as_ref(), state.redis_pool, api_key)
            .await
            .map_err(ApiError::from)?;

    let auth_config = Arc::new(auth_config);

    tracing::debug!(
        app_id = %auth_config.app_id,
        developer_id = %auth_config.app_user_id,
        "API key validated successfully"
    );

    req.extensions_mut()
        .insert(ApplicationKeyViewExt(auth_config.clone()));

    // Store the auth cache key for use in handlers
    req.extensions_mut().insert(AuthCacheKey(auth_cache_key));

    Ok(next.run(req).await)
}

impl FromRequestParts<AppState> for ApplicationKeyViewExt {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ApplicationKeyViewExt>()
            .cloned()
            .ok_or_else(|| {
                ApiError::unauthorized("Missing API key authentication")
                    .with_code("MISSING_API_KEY_AUTH")
            })
    }
}

impl FromRequestParts<AppState> for AuthCacheKey {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthCacheKey>()
            .cloned()
            .ok_or_else(|| {
                ApiError::unauthorized("Missing API key authentication")
                    .with_code("MISSING_API_KEY_AUTH")
            })
    }
}
