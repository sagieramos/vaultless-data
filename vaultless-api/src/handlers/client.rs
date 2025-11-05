use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    middleware::{
        client::{AuthenticatedClient, AuthenticatedClientWithToken},
        error::ApiError,
    },
    state::AppState,
};
use vaultless_core::{
    AuthenticateClientRequest, AuthenticateClientResponse, Client, ClientPublic,
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
    pub client: Option<ClientPublic>,
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
#[axum::debug_handler]
pub async fn register_client(
    State(state): State<AppState>,
    Json(input): Json<RegisterClientRequest>,
) -> Result<Json<RegisterClientResponse>, ApiError> {
    tracing::info!("Client registration attempt");

    // Call secure register with Redis (for nonce & caching)
    let response = Client::register(
        state.db.as_ref(),
        input,
        None,                           // developer_id - extract from API key context if needed
        None,                           // api_key_id - extract from API key context if needed
        Some(state.redis_pool.clone()), // arg for nonce replay protection + caching
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!("Client registered successfully: {}", response.client_id);

    Ok(Json(response))
}

/// Authenticate existing client (issue new session or refresh)
/// POST /api/clients/authenticate
#[axum::debug_handler]
pub async fn authenticate_client(
    State(state): State<AppState>,
    Json(input): Json<AuthenticateClientRequest>,
) -> Result<Json<AuthenticateClientResponse>, ApiError> {
    tracing::info!("Client authentication attempt");

    let response = Client::authenticate(state.db.as_ref(), input).await?;

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
pub async fn generate_challenge() -> Result<Json<ChallengeResponse>, ApiError> {
    let challenge = Client::generate_challenge()?;

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
) -> Json<ClientPublic> {
    Json(client)
}

/// Logout (revoke current session)
/// POST /api/clients/logout
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
