use crate::{
    middleware::{
        application::ApplicationKeyViewExt,
        client::{ClientResponse, SessionDataClientExt},
        error::ApiError,
    },
    state::AppState,
};
use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
};
use hyper::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json;
use utoipa::{IntoParams, ToSchema};
use vaultless_core::{
    Client, LoginClientRequest, LoginClientResponse, SignupClientRequest, SignupClientResponse,
};

// =============================================================================
// Request/Response Types
// =============================================================================

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct LookupClientQuery {
    pub identifier: Option<String>,
    pub pubkey: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ClientLookupResponse {
    pub success: bool,
    #[schema(value_type = Option<Object>)]
    pub client: Option<Client>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ChallengeResponse {
    pub challenge: String,
    pub expires_at: String,
}

// =============================================================================
// Public Endpoints (No Authentication Required)
// =============================================================================

/// Register new client with optional platform attestation
/// POST /api/clients/register
#[utoipa::path(
    post,
    path = "/api/clients/register",
    tag = "Client Authentication",
    request_body(content = Object, description = "Client registration request"),
    responses(
        (status = 200, description = "Client registered successfully", body = Object),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
#[axum::debug_handler]
pub async fn sign_up_client(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    ApplicationKeyViewExt(auth_config): ApplicationKeyViewExt,
    Json(input): Json<SignupClientRequest>,
) -> Result<Json<SignupClientResponse>, ApiError> {
    let response = Client::sign_up_hybrid(
        state.db.as_ref(),
        state.redis_pool,
        state.session_verifier_hybrid,
        auth_config,
        state.attestation_service,
        input,
        addr.ip(),
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
#[utoipa::path(
    post,
    path = "/api/clients/login",
    tag = "Client Authentication",
    request_body(content = Object, description = "Client login request"),
    responses(
        (status = 200, description = "Client logged in successfully", body = Object),
        (status = 401, description = "Invalid credentials"),
        (status = 500, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state, input), fields(endpoint = "login_client"))]
#[axum::debug_handler]
pub async fn login_client(
    State(state): State<AppState>,
    ApplicationKeyViewExt(auth_config): ApplicationKeyViewExt,
    Json(input): Json<LoginClientRequest>,
) -> Result<Json<LoginClientResponse>, ApiError> {
    let response = Client::login_hybrid(
        state.db.as_ref(),
        state.redis_pool,
        state.session_verifier_hybrid,
        auth_config,
        state.attestation_service,
        input,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %response.client_id,
        was_reattested = %response.was_reattested,
        "Client logged in successfully"
    );

    Ok(Json(response))
}

/// Generate authentication challenge (for signature-based auth)
/// GET /api/clients/challenge
#[utoipa::path(
    get,
    path = "/api/clients/challenge",
    tag = "Client Authentication",
    responses(
        (status = 200, description = "Challenge generated successfully", body = ChallengeResponse),
        (status = 500, description = "Internal server error")
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/clients/lookup",
    tag = "Client Authentication",
    params(LookupClientQuery),
    responses(
        (status = 200, description = "Client lookup result", body = ClientLookupResponse),
        (status = 500, description = "Internal server error")
    )
)]
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
        None,
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
#[utoipa::path(
    get,
    path = "/api/clients/health",
    tag = "Client Authentication",
    responses(
        (status = 200, description = "Health check status", body = Object)
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/clients/me",
    tag = "Client Authentication",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Current client info", body = ClientResponse),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn get_current_client(client: ClientResponse) -> Json<ClientResponse> {
    Json(client)
}

/// Logout (revoke current session)
/// POST /api/clients/logout
#[utoipa::path(
    post,
    path = "/api/clients/logout",
    tag = "Client Authentication",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Logged out successfully", body = Object),
        (status = 401, description = "Unauthorized")
    )
)]
#[tracing::instrument(skip(state, headers), fields(endpoint = "logout_client"))]
#[axum::debug_handler]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = crate::middleware::helper::extract_bearer_token(&headers)?;

    // Use SECURE verification for logout (bypasses cache)
    let session_data = state
        .session_verifier_hybrid
        .verify_secure(token)
        .await
        .map_err(ApiError::from)?;

    Client::revoke_client_session_with_hybrid_verifier(
        state.db.as_ref(),
        state.session_verifier_hybrid,
        session_data.client_id,
        Some(token),
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %session_data.client_id,
        "Client logged out successfully"
    );

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Logged out successfully"
    })))
}

/// Deactivate client account
/// DELETE /api/clients/me
#[utoipa::path(
    delete,
    path = "/api/clients/me",
    tag = "Client Authentication",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Client deactivated successfully", body = SuccessResponse),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn deactivate_client(
    State(state): State<AppState>,
    SessionDataClientExt(session_data): SessionDataClientExt,
) -> Result<Json<SuccessResponse>, ApiError> {
    Client::revoke_client_session_with_hybrid_verifier(
        state.db.as_ref(),
        state.session_verifier_hybrid.clone(),
        session_data.client_id,
        None,
    )
    .await
    .map_err(ApiError::from)?;

    Client::deactivate_with_hybrid_verifier(
        state.db.as_ref(),
        Some(&state.redis_pool),
        state.session_verifier_hybrid,
        session_data.client_id,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!("Client {} deactivated", session_data.client_id);

    Ok(Json(SuccessResponse {
        success: true,
        message: "Client deactivated successfully".to_string(),
    }))
}
