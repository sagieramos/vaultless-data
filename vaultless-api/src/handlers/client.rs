use axum::{
    Json,
    extract::{Query, State},
};
use chrono::Utc;
use hyper::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::{
    middleware::{
        client::{AuthenticatedClient, AuthenticatedClientWithToken},
        error::ApiError,
    },
    state::AppState,
};
use vaultless_core::{
    Application, AuthenticateClientRequest, AuthenticateClientResponse, Client,
    RegisterClientRequest, RegisterClientResponse,
};

// =============================================================================
// Request/Response Types
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct LookupClientQuery {
    pub identifier: Option<String>,
    pub pubkey: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClientLookupResponse {
    pub success: bool,
    pub client: Option<Client>,
}

#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    pub challenge: String,
    pub expires_at: String,
}

// =============================================================================
// Public Endpoints (No Authentication Required)
// =============================================================================

/// Register a new anonymous client
/// POST /api/clients/register
#[tracing::instrument(skip(state, input), fields(endpoint = "register_client"))]
#[axum::debug_handler]
pub async fn register_client(
    State(state): State<AppState>,
    TypedHeader(publishable_key): TypedHeader<XPublishableKey>, 
    Json(input): Json<RegisterClientRequest>,
) -> Result<Json<RegisterClientResponse>, ApiError> {
    tracing::info!("Client registration attempt");

    let app = Application::find_by_publishable_key(
        state.db.as_ref(), // Changes from &state.db to state.db.as_ref()
        Some(state.redis.clone()),
        &publishable_key.0,
    )
    .await?;

    // Call secure register with Redis (for nonce & caching)
    let response = Client::register(state.db.as_ref(), Some(state.redis_pool.clone()), input)
        .await
        .map_err(ApiError::from)?;

    tracing::info!("Client registered successfully: {}", response.client_id);

    Ok(Json(response))
}

/// Authenticate existing client (issue new session or refresh)
/// POST /api/clients/authenticate
#[tracing::instrument(skip(state, headers, input), fields(endpoint = "authenticate_client"))]
#[axum::debug_handler]
pub async fn authenticate_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AuthenticateClientRequest>,
) -> Result<Json<AuthenticateClientResponse>, ApiError> {
    tracing::info!("Client authentication attempt");

    // 1️⃣ Try verifying existing session token
    if let Some(auth_header) = headers.get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            let auth_str = auth_str.trim();
            if let Some(token) = auth_str
                .strip_prefix("Bearer ")
                .or_else(|| auth_str.strip_prefix("bearer "))
            {
                let token = token.trim();

                match Client::verify_session(&*state.db, Some(state.redis_pool.clone()), token)
                    .await
                {
                    Ok(verified) => {
                        tracing::info!("Valid session for client {}", verified.id);

                        let response = AuthenticateClientResponse {
                            client_id: verified.id,
                            session_token: "".to_string(),
                            expires_at: verified.session_expires_at.unwrap_or_else(|| Utc::now()),
                            is_new_session: false,
                        };
                        return Ok(Json(response));
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Session invalid: {e}. Falling back to challenge-based auth."
                        );
                    }
                }
            }
        }
    }

    // 2️⃣ Fallback to challenge-based authentication
    let response = Client::authenticate(state.db.as_ref(), state.redis_pool.clone(), input)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        "Client authenticated: {} (new_session: {})",
        response.client_id,
        response.is_new_session
    );

    Ok(Json(response))
}

/// Generate authentication challenge (for signature-based auth)
/// GET /api/clients/challenge
#[axum::debug_handler]
pub async fn generate_challenge(
    State(state): State<AppState>,
) -> Result<Json<ChallengeResponse>, ApiError> {
    let challenge = Client::generate_and_cache_challenge(state.redis_pool)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ChallengeResponse {
        challenge: challenge.challenge,
        expires_at: challenge.expires_at.to_rfc3339(),
    }))
}

/// Lookup client by identifier (public or hashed)
/// GET /api/clients/lookup?identifier=<identifier>&pubkey=<pubkey>
#[axum::debug_handler]
pub async fn lookup_client(
    State(state): State<AppState>,
    Query(query): Query<LookupClientQuery>,
) -> Result<Json<ClientLookupResponse>, ApiError> {
    tracing::debug!(
        "Client lookup: identifier={:?}, pubkey={:?}",
        query.identifier,
        query.pubkey
    );

    let client = Client::resolve_client(
        state.db.as_ref(),
        Some(state.redis_pool.clone()),
        query.pubkey.as_deref(),
        query.identifier.as_deref(),
        None, // client_identifier is never passed from the query
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(ClientLookupResponse {
        success: client.is_some(),
        client,
    }))
}

/// Health check endpoint
/// GET /api/clients/health
#[axum::debug_handler]
pub async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "client-auth"
    }))
}

// =============================================================================
// Protected Endpoints (Authentication Required)
// =============================================================================

/// Get current authenticated client info
/// GET /api/clients/me
#[axum::debug_handler]
pub async fn get_current_client(
    State(_state): State<AppState>,
    AuthenticatedClient(client): AuthenticatedClient,
) -> Json<Client> {
    Json(client)
}

/// Logout (revoke current session)
/// POST /api/clients/logout
#[tracing::instrument(skip(state, client, token), fields(endpoint = "logout_client", client_id = %client.id))]
#[axum::debug_handler]
pub async fn logout_client(
    State(state): State<AppState>,
    AuthenticatedClientWithToken { client, token }: AuthenticatedClientWithToken,
) -> Result<Json<SuccessResponse>, ApiError> {
    Client::revoke_session(
        state.db.as_ref(),
        Some(&state.redis_pool),
        client.id,
        Some(&token),
    )
    .await?;

    tracing::info!("Client {} logged out", client.id);

    Ok(Json(SuccessResponse {
        success: true,
        message: "Session revoked successfully".to_string(),
    }))
}

/// Deactivate client account
/// DELETE /api/clients/me
#[axum::debug_handler]
pub async fn deactivate_client(
    State(state): State<AppState>,
    AuthenticatedClient(client): AuthenticatedClient,
) -> Result<Json<SuccessResponse>, ApiError> {
    Client::deactivate(state.db.as_ref(), Some(&state.redis_pool), client.id)
        .await
        .map_err(ApiError::from)?; // map your VaultlessError to ApiError

    tracing::info!("Client {} deactivated", client.id);

    Ok(Json(SuccessResponse {
        success: true,
        message: "Client deactivated successfully".to_string(),
    }))
}

// Custom header extractor
#[derive(Debug, Clone)]
pub struct XPublishableKey(pub String);

impl<S> axum::extract::FromRequestParts<S> for XPublishableKey
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("X-Publishable-Key")
            .and_then(|v| v.to_str().ok())
            .map(|s| XPublishableKey(s.to_string()))
            .ok_or_else(|| ApiError::unauthorized("X-Publishable-Key header required".into()))
    }
}
