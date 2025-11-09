use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{middleware::error::ApiError, state::AppState};
use vaultless_core::{Client, Application};

/// Extractor for authenticated clients
/// Usage: `async fn handler(AuthenticatedClient(client): AuthenticatedClient)`
pub struct AuthenticatedClient(pub Client);

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
        let client = Client::verify_session(&*app_state.db, Some(app_state.redis_pool), token)
            .await
            .map_err(|e| {
                tracing::warn!("Session verification failed: {}", e);
                ApiError::from(e)
            })?;

        Ok(AuthenticatedClient(client))
    }
}


// Validated Application Extractor (Recommended)

#[derive(Debug, Clone)]
pub struct ValidatedApplication(pub Application);

impl<S> FromRequestParts<S> for ValidatedApplication
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let publishable_key = parts
            .headers
            .get("X-Publishable-Key")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ApiError::unauthorized("Missing X-Publishable-Key header")
                    .with_code("MISSING_PUBLISHABLE_KEY")
            })?;

        if !publishable_key.starts_with("pk_") {
            return Err(
                ApiError::unauthorized("Invalid publishable key format")
                    .with_code("INVALID_PUBLISHABLE_KEY_FORMAT")
            );
        }

        let app_state: AppState = axum::extract::FromRef::from_ref(state);

        let app = Application::find_by_publishable_key(
            &*app_state.db,
            Some(app_state.redis_pool.clone()),
            publishable_key,
        )
        .await
        .map_err(|e| {
            tracing::warn!("Publishable key validation failed: {}", e);
            ApiError::unauthorized("Invalid or inactive publishable key")
                .with_code("INVALID_PUBLISHABLE_KEY")
        })?;

        if !app.is_active {
            return Err(
                ApiError::unauthorized("Application is deactivated")
                    .with_code("APPLICATION_INACTIVE")
            );
        }

        Ok(ValidatedApplication(app))
    }
}

/// Extractor with token included (for logout operations)
pub struct AuthenticatedClientWithToken {
    pub client: Client,
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

        let client = Client::verify_session(&*app_state.db, Some(app_state.redis_pool), &token)
            .await
            .map_err(|e| {
                tracing::warn!("Session verification failed: {}", e);
                ApiError::from(e)
            })?;

        Ok(AuthenticatedClientWithToken { client, token })
    }
}

/// Optional extractor - doesn't fail if no auth header present
pub struct OptionalAuthenticatedClient(pub Option<Client>);

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
        match Client::verify_session(&*app_state.db, Some(app_state.redis_pool), token).await {
            Ok(client) => Ok(OptionalAuthenticatedClient(Some(client))),
            Err(e) => {
                tracing::debug!("Optional auth failed (non-critical): {}", e);
                Ok(OptionalAuthenticatedClient(None))
            }
        }
    }
}

// Simple Publishable Key Extractor
#[derive(Debug, Clone)]
pub struct XPublishableKey(pub String);

impl<S> FromRequestParts<S> for XPublishableKey
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let publishable_key = parts
            .headers
            .get("X-Publishable-Key")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                ApiError::unauthorized("Missing X-Publishable-Key header")
                    .with_code("MISSING_PUBLISHABLE_KEY")
            })?;

        // Basic validation
        if !publishable_key.starts_with("pk_") {
            return Err(
                ApiError::unauthorized("Invalid publishable key format")
                    .with_code("INVALID_PUBLISHABLE_KEY_FORMAT")
            );
        }

        Ok(XPublishableKey(publishable_key.to_string()))
    }
}
