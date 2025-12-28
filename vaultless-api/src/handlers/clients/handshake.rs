//! Handshake endpoints for establishing secure sessions between clients
//!
//! Client-side cryptographic handshake protocol:
//! 1. Initiator calls /initiate to lookup peer metadata
//! 2. Initiator generates handshake request client-side (with private key)
//! 3. Responder receives request, generates response client-side, calls /respond to store session
//! 4. Initiator receives response, derives session key client-side, calls /complete to store session
//!
//! All cryptographic operations (signing, key derivation) happen client-side.
//! Server only stores session metadata for message routing.

use axum::{extract::State, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::{
    middleware::{client::SessionDataClientExt, error::ApiError},
    state::AppState,
};
use vaultless_core::{
    models::session_keys::{CreateSessionKeyRequest, SessionKey},
    Client,
};

// =============================================================================
// DTOs
// =============================================================================

/// Request to lookup peer for handshake initiation
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct HandshakeInitiateRequest {
    /// Peer client identifier or public key to establish session with
    #[validate(length(min = 1, max = 1024))]
    pub peer_identifier: Option<String>,

    /// Peer client signing public key (alternative to identifier)
    #[validate(length(min = 32, max = 1024))]
    pub peer_signing_key: Option<String>,
}

/// Response containing peer metadata for client-side handshake generation
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeInitiateResponse {
    /// Peer's signing public key (Ed25519)
    pub peer_signing_key: String,

    /// Peer's identifier (if available)
    pub peer_identifier: Option<String>,
}

/// Serializable handshake request data
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeRequestData {
    pub handshake_id: String,
    pub signing_pubkey: String,
    pub ephemeral_exchange_pubkey: String,
    pub timestamp: chrono::DateTime<Utc>,
    pub signature: String,
}

/// Request to validate and store session after handshake (responder side)
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct HandshakeRespondRequest {
    /// Handshake request from initiator (for verification)
    pub handshake_request: HandshakeRequestData,

    /// Session ID generated during handshake
    #[validate(length(min = 1, max = 128))]
    pub session_id: String,

    /// Responder's ephemeral X25519 public key for this session (base64)
    #[validate(length(min = 32, max = 128))]
    pub ephemeral_public_key: String,

    /// Session expiry time
    pub expires_at: chrono::DateTime<Utc>,
}

/// Response after storing responder's session
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeRespondResponse {
    /// Session ID
    pub session_id: String,

    /// Session expiry time
    pub expires_at: chrono::DateTime<Utc>,
}

/// Serializable handshake response data
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeResponseData {
    pub handshake_id: String,
    pub signing_pubkey: String,
    pub ephemeral_exchange_pubkey: String,
    pub timestamp: chrono::DateTime<Utc>,
    pub session_id: String,
    pub expires_at: chrono::DateTime<Utc>,
    pub signature: String,
}

/// Request to store session after handshake completion (initiator side)
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct HandshakeCompleteRequest {
    /// Handshake response from responder (for verification)
    pub handshake_response: HandshakeResponseData,

    /// Expected handshake ID (for validation)
    #[validate(length(min = 1, max = 128))]
    pub expected_handshake_id: String,

    /// Initiator's ephemeral X25519 public key for this session (base64)
    #[validate(length(min = 32, max = 128))]
    pub ephemeral_public_key: String,
}

/// Response after storing initiator's session
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeCompleteResponse {
    /// Session ID
    pub session_id: String,

    /// Session expiry time
    pub expires_at: chrono::DateTime<Utc>,
}

// =============================================================================
// Handlers
// =============================================================================

/// Lookup peer for handshake initiation
///
/// Returns peer metadata needed for client-side handshake generation.
/// The client generates the handshake request locally using their private key.
#[utoipa::path(
    post,
    path = "/api/v1/clients/handshake/initiate",
    tag = "Session Management",
    security(("bearer_auth" = [])),
    request_body = HandshakeInitiateRequest,
    responses(
        (status = 200, description = "Peer found, ready for handshake", body = HandshakeInitiateResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Clients in different applications"),
        (status = 404, description = "Peer client not found")
    )
)]
pub async fn initiate_handshake(
    State(state): State<AppState>,
    SessionDataClientExt(session): SessionDataClientExt,
    Json(input): Json<HandshakeInitiateRequest>,
) -> Result<Json<HandshakeInitiateResponse>, ApiError> {
    // Validate input
    input
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Get initiator to validate application
    let initiator = Client::fetch_active_client(
        state.db.as_ref(),
        &state.redis_pool,
        session.client_id,
    )
    .await
    .map_err(ApiError::from)?;

    // Resolve peer client
    let peer = Client::resolve_client(
        state.db.as_ref(),
        Some(state.redis_pool.clone()),
        input.peer_signing_key.as_deref(),
        input.peer_identifier.as_deref(),
        None,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Peer client not found"))?;

    // Validate both clients are in the same application
    if initiator.application_id != peer.application_id {
        return Err(ApiError::forbidden(
            "Cannot establish session with client from different application",
        ));
    }

    let peer_signing_key = peer
        .signing_key
        .ok_or_else(|| ApiError::bad_request("Peer signing key not found"))?;

    Ok(Json(HandshakeInitiateResponse {
        peer_signing_key,
        peer_identifier: peer.identifier,
    }))
}

/// Store session after handshake response (responder side)
///
/// Verifies the handshake request signature and stores the responder's session key entry.
/// The client generates the handshake response locally using their private key.
#[utoipa::path(
    post,
    path = "/api/v1/clients/handshake/respond",
    tag = "Session Management",
    security(("bearer_auth" = [])),
    request_body = HandshakeRespondRequest,
    responses(
        (status = 200, description = "Session stored successfully", body = HandshakeRespondResponse),
        (status = 400, description = "Invalid request or signature verification failed"),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn respond_to_handshake(
    State(state): State<AppState>,
    SessionDataClientExt(session): SessionDataClientExt,
    Json(input): Json<HandshakeRespondRequest>,
) -> Result<Json<HandshakeRespondResponse>, ApiError> {
    // Validate input
    input
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Get responder
    let responder = Client::fetch_active_client(
        state.db.as_ref(),
        &state.redis_pool,
        session.client_id,
    )
    .await
    .map_err(ApiError::from)?;

    // Resolve initiator client by signing key
    // NOTE: Signature verification is done client-side before calling this endpoint
    let initiator = Client::resolve_client(
        state.db.as_ref(),
        Some(state.redis_pool.clone()),
        Some(&input.handshake_request.signing_pubkey),
        None,
        None,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Initiator client not found"))?;

    // Validate both clients are in the same application
    if responder.application_id != initiator.application_id {
        return Err(ApiError::forbidden(
            "Cannot establish session with client from different application",
        ));
    }

    // Store session key in database (responder side)
    let session_req = CreateSessionKeyRequest {
        client_id: session.client_id,
        peer_client_id: initiator.id,
        application_id: session.application_id,
        session_id: input.session_id.clone(),
        ephemeral_public_key: input.ephemeral_public_key.clone(),
        expires_at: input.expires_at,
    };

    SessionKey::create(state.db.as_ref(), session_req)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %session.client_id,
        peer_id = %initiator.id,
        session_id = %input.session_id,
        expires_at = %input.expires_at,
        "Responder session stored successfully"
    );

    Ok(Json(HandshakeRespondResponse {
        session_id: input.session_id,
        expires_at: input.expires_at,
    }))
}

/// Store session after handshake completion (initiator side)
///
/// Verifies the handshake response signature and stores the initiator's session key entry.
/// The client derives the session key locally using their ephemeral private key.
#[utoipa::path(
    post,
    path = "/api/v1/clients/handshake/complete",
    tag = "Session Management",
    security(("bearer_auth" = [])),
    request_body = HandshakeCompleteRequest,
    responses(
        (status = 200, description = "Session stored successfully", body = HandshakeCompleteResponse),
        (status = 400, description = "Invalid request or signature verification failed"),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn complete_handshake(
    State(state): State<AppState>,
    SessionDataClientExt(session): SessionDataClientExt,
    Json(input): Json<HandshakeCompleteRequest>,
) -> Result<Json<HandshakeCompleteResponse>, ApiError> {
    // Validate input
    input
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Verify handshake ID matches
    if input.handshake_response.handshake_id != input.expected_handshake_id {
        return Err(ApiError::bad_request("Handshake ID mismatch"));
    }

    // Resolve peer client by signing key
    // NOTE: Signature verification is done client-side before calling this endpoint
    let peer = Client::resolve_client(
        state.db.as_ref(),
        Some(state.redis_pool.clone()),
        Some(&input.handshake_response.signing_pubkey),
        None,
        None,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Peer client not found"))?;

    // Get initiator to validate application
    let initiator = Client::fetch_active_client(
        state.db.as_ref(),
        &state.redis_pool,
        session.client_id,
    )
    .await
    .map_err(ApiError::from)?;

    // Validate both clients are in the same application
    if initiator.application_id != peer.application_id {
        return Err(ApiError::forbidden(
            "Cannot establish session with client from different application",
        ));
    }

    // Store session key in database (initiator side)
    let session_req = CreateSessionKeyRequest {
        client_id: session.client_id,
        peer_client_id: peer.id,
        application_id: session.application_id,
        session_id: input.handshake_response.session_id.clone(),
        ephemeral_public_key: input.ephemeral_public_key.clone(),
        expires_at: input.handshake_response.expires_at,
    };

    SessionKey::create(state.db.as_ref(), session_req)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %session.client_id,
        peer_id = %peer.id,
        session_id = %input.handshake_response.session_id,
        expires_at = %input.handshake_response.expires_at,
        "Initiator session stored successfully"
    );

    Ok(Json(HandshakeCompleteResponse {
        session_id: input.handshake_response.session_id,
        expires_at: input.handshake_response.expires_at,
    }))
}
