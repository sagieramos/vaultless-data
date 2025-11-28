use crate::{
    middleware::{
        client::{AuthConfigExt, ClientExt, SessionDataClientExt},
        error::ApiError,
    },
    state::AppState,
};
use axum::{
    Json,
    extract::{Query, State},
};
use chrono::Utc;
use hyper::HeaderMap;
use serde::{Deserialize, Serialize};
use vaultless_core::models::session::extract_token_expiration;
use vaultless_core::{
    AuthenticateClientRequest, AuthenticateClientResponse, Client, RegisterClientRequest,
    RegisterClientResponse,
};

use vaultless_core::models::session::paseto_session::verify_session_token;

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

/// Register new client with optional platform attestation
/// POST /api/clients/register
#[axum::debug_handler]
pub async fn register_client(
    State(state): State<AppState>,
    AuthConfigExt(auth_config): AuthConfigExt,
    Json(input): Json<RegisterClientRequest>,
) -> Result<Json<RegisterClientResponse>, ApiError> {
    tracing::info!(
        platform = ?input.attestation.as_ref().map(|a| &a.platform),
        has_attestation = input.attestation.is_some(),
        "Client registration attempt"
    );

    let response = Client::sign_up(
        state.db.as_ref(),
        Some(state.redis_pool),
        state.session_key_manager,
        state.attestation_service,
        input,
        auth_config,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %response.client_id,
        "Client registered successfully"
    );

    Ok(Json(response))
}

/// Authenticate existing client (issue new session or refresh)
/// POST /api/clients/login

/// Login handler - uses fast verification
#[tracing::instrument(skip(state, headers, input), fields(endpoint = "login_client"))]
#[axum::debug_handler]
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    AuthConfigExt(auth_config): AuthConfigExt,
    Json(input): Json<AuthenticateClientRequest>,
) -> Result<Json<AuthenticateClientResponse>, ApiError> {
    tracing::info!(
        application_id = %auth_config.app_id,
        "Client authentication attempt"
    );

    // 1 Try verifying existing session token (FAST PATH with caching)
    if let Ok(token) = crate::middleware::helper::extract_bearer_token(&headers) {
        if let Ok(session_data) = state.session_verifier.verify_fast(token).await {
            tracing::info!(
                client_id = %session_data.client_id,
                device_trusted = session_data.device_trusted,
                platform = %session_data.platform,
                "Valid session found - reusing token"
            );

            // Extract expiration only when needed
            let expires_at = extract_token_expiration(&state.session_key_manager, token)
                .unwrap_or_else(|_| Utc::now() + chrono::Duration::days(30));

            let response = AuthenticateClientResponse {
                client_id: session_data.client_id,
                session_token: token.to_string(),
                expires_at,
                is_new_session: false,
                was_reattested: false,
            };

            return Ok(Json(response));
        }
    }

    // 2 Challenge-based authentication
    tracing::info!("Authenticating with challenge-based flow");

    let response = Client::login(
        state.db.as_ref(),
        state.redis_pool.clone(),
        state.session_key_manager.clone(),
        auth_config,
        state.attestation_service.clone(),
        input,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %response.client_id,
        was_reattested = %response.was_reattested,
        "Client authenticated successfully"
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
pub async fn get_current_client(ClientExt(client): ClientExt) -> Json<Client> {
    Json(client)
}

/// Logout (revoke current session)
/// POST /api/clients/logout
/// Logout handler - uses secure verification
#[tracing::instrument(skip(state, headers), fields(endpoint = "logout_client"))]
#[axum::debug_handler]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = crate::middleware::helper::extract_bearer_token(&headers)?;

    // Use SECURE verification for logout (bypasses cache)
    let session_data = state
        .session_verifier
        .verify_secure(token)
        .await
        .map_err(ApiError::from)?;

    // Extract JTI for revocation
    let (_, jti) =
        verify_session_token(&state.session_key_manager, token).map_err(ApiError::from)?;

    // Revoke session (broadcasts to all nodes)
    state
        .session_verifier
        .revoke_session(&jti, 2592000)
        .await // 30 days TTL
        .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %session_data.client_id,
        jti = %jti,
        "Client logged out successfully"
    );

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Logged out successfully"
    })))
}

/// Deactivate client account
/// DELETE /api/clients/me
pub async fn deactivate_client(
    State(state): State<AppState>,
    SessionDataClientExt(session_data): SessionDataClientExt,
) -> Result<Json<SuccessResponse>, ApiError> {
    Client::revoke_client_session(
        state.db.as_ref(),
        Some(&state.redis_pool),
        &state.session_key_manager,
        session_data.client_id,
        None,
    )
    .await?;
    Client::deactivate(
        state.db.as_ref(),
        Some(&state.redis_pool),
        session_data.client_id,
    )
    .await?;

    tracing::info!("Client {} deactivated", session_data.client_id);

    Ok(Json(SuccessResponse {
        success: true,
        message: "Client deactivated successfully".to_string(),
    }))
}
