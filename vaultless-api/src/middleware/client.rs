use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};

use crate::{middleware::error::ApiError, state::AppState};
use vaultless_core::{Client, ClientPublic};

/// Extractor for authenticated clients
/// Usage: `async fn handler(AuthenticatedClient(client): AuthenticatedClient)`
pub struct AuthenticatedClient(pub ClientPublic);

impl<S> FromRequestParts<S> for AuthenticatedClient
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Extract session token from Authorization header
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ApiError::unauthorized("Missing Authorization header")
                    .with_code("MISSING_AUTH_HEADER")
            })?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            ApiError::unauthorized("Invalid Authorization format. Expected: Bearer <token>")
                .with_code("INVALID_AUTH_FORMAT")
        })?;

        // Get app state
        let app_state: AppState = axum::extract::FromRef::from_ref(state);

        // Verify session (converts VaultlessError to ApiError automatically)
        let client = Client::verify_session(
            &app_state.db,
            Some(app_state.redis_pool.clone().into()),
            token,
        )
        .await
        .map_err(|e| {
            tracing::warn!("Session verification failed: {}", e);
            ApiError::from(e)
        })?;

        Ok(AuthenticatedClient(client))
    }
}

/// Extractor with token included (for logout operations)
pub struct AuthenticatedClientWithToken {
    pub client: ClientPublic,
    pub token: String,
}

impl<S> FromRequestParts<S> for AuthenticatedClientWithToken
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ApiError::unauthorized("Missing Authorization header")
                    .with_code("MISSING_AUTH_HEADER")
            })?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| {
                ApiError::unauthorized("Invalid Authorization format")
                    .with_code("INVALID_AUTH_FORMAT")
            })?
            .to_string();

        let app_state: AppState = axum::extract::FromRef::from_ref(state);

        let client = Client::verify_session(
            &app_state.db,
            Some(app_state.redis_pool.clone().into()),
            &token,
        )
        .await
        .map_err(|e| {
            tracing::warn!("Session verification failed: {}", e);
            ApiError::from(e)
        })?;

        Ok(AuthenticatedClientWithToken { client, token })
    }
}

/// Optional extractor - doesn't fail if no auth header present
pub struct OptionalAuthenticatedClient(pub Option<ClientPublic>);

impl<S> FromRequestParts<S> for OptionalAuthenticatedClient
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Try to extract Authorization header
        let auth_header = match parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
        {
            Some(header) => header,
            None => return Ok(OptionalAuthenticatedClient(None)),
        };

        let token = match auth_header.strip_prefix("Bearer ") {
            Some(t) => t,
            None => return Ok(OptionalAuthenticatedClient(None)),
        };

        // Get app state
        let app_state: AppState = axum::extract::FromRef::from_ref(state);

        // Try to verify session
        match Client::verify_session(
            &app_state.db,
            Some(app_state.redis_pool.clone().into()),
            token,
        )
        .await
        {
            Ok(client) => Ok(OptionalAuthenticatedClient(Some(client))),
            Err(e) => {
                tracing::debug!("Optional auth failed (non-critical): {}", e);
                Ok(OptionalAuthenticatedClient(None))
            }
        }
    }
}
