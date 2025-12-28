//! Handshake endpoints for establishing secure sessions between clients
//!
//! Implements the cryptographic handshake protocol:
//! 1. Initiator sends HandshakeRequest with ephemeral X25519 public key
//! 2. Responder verifies and responds with their ephemeral X25519 public key
//! 3. Both derive shared session key via ECDH + HKDF
//! 4. Session keys stored in database for message routing

use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::{
    middleware::{client::SessionDataClientExt, error::ApiError},
    state::AppState,
};
use vaultless_core::{
    crypto::{handshake, keys},
    models::session_keys::{CreateSessionKeyRequest, SessionKey},
    Client,
};

// =============================================================================
// DTOs
// =============================================================================

/// Request to initiate a handshake with another client
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct HandshakeInitiateRequest {
    /// Peer client identifier or public key to establish session with
    #[validate(length(min = 1, max = 1024))]
    pub peer_identifier: Option<String>,

    /// Peer client signing public key (alternative to identifier)
    #[validate(length(min = 32, max = 1024))]
    pub peer_signing_key: Option<String>,

    /// Initiator's ephemeral X25519 public key for this session (base64)
    #[validate(length(min = 32, max = 128))]
    pub ephemeral_public_key: String,

    /// Session duration in minutes (default: 60, max: 1440 = 24 hours)
    pub session_duration_minutes: Option<i64>,
}

/// Response containing handshake request to send to peer
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeInitiateResponse {
    /// Unique handshake ID for tracking
    pub handshake_id: String,

    /// Handshake request to send to peer (serialized)
    pub handshake_request: HandshakeRequestData,

    /// Peer client ID
    pub peer_client_id: Uuid,
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

/// Request to respond to a handshake
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct HandshakeRespondRequest {
    /// Handshake request from initiator
    pub handshake_request: HandshakeRequestData,

    /// Responder's ephemeral X25519 public key for this session (base64)
    #[validate(length(min = 32, max = 128))]
    pub ephemeral_public_key: String,

    /// Session duration in minutes (default: 60, max: 1440 = 24 hours)
    pub session_duration_minutes: Option<i64>,
}

/// Response containing handshake response and session metadata
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeRespondResponse {
    /// Session ID
    pub session_id: String,

    /// Session expiry time
    pub expires_at: chrono::DateTime<Utc>,

    /// Handshake response to send back to initiator
    pub handshake_response: HandshakeResponseData,

    /// Initiator's client ID
    pub initiator_client_id: Uuid,
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

/// Request to complete a handshake (initiator receives response)
#[derive(Debug, Clone, Serialize, Deserialize, Validate, ToSchema)]
pub struct HandshakeCompleteRequest {
    /// Handshake response from responder
    pub handshake_response: HandshakeResponseData,

    /// Expected handshake ID (for validation)
    #[validate(length(min = 1, max = 128))]
    pub expected_handshake_id: String,

    /// Initiator's ephemeral X25519 private key (base64, will be used client-side only)
    /// NOTE: This should NOT be sent over the wire in production
    /// This endpoint is for demonstration - real implementation derives key client-side
    #[validate(length(min = 32, max = 128))]
    pub ephemeral_private_key: String,
}

/// Response after completing handshake
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HandshakeCompleteResponse {
    /// Session ID
    pub session_id: String,

    /// Derived session key (base64, 32 bytes)
    /// NOTE: In production, this should be derived client-side and never sent over wire
    pub session_key: String,

    /// Session expiry time
    pub expires_at: chrono::DateTime<Utc>,

    /// Peer's signing public key
    pub peer_signing_pubkey: String,
}

// =============================================================================
// Handlers
// =============================================================================

/// Initiate a handshake with another client
///
/// Creates a signed handshake request with an ephemeral X25519 public key.
/// The request should be sent to the peer client via the messaging system.
#[utoipa::path(
    post,
    path = "/api/v1/clients/handshake/initiate",
    tag = "Session Management",
    security(("bearer_auth" = [])),
    request_body = HandshakeInitiateRequest,
    responses(
        (status = 200, description = "Handshake initiated successfully", body = HandshakeInitiateResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
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

    // Get initiator's signing key from database
    let initiator = Client::fetch_active_client(
        state.db.as_ref(),
        &state.redis_pool,
        session.client_id,
    )
    .await
    .map_err(ApiError::from)?;

    let signing_key = initiator
        .signing_key
        .ok_or_else(|| ApiError::bad_request("Client signing key not found"))?;

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

    // Create signing keypair (we only have public key, so we'll create a simplified version)
    // NOTE: In production, the client should generate the handshake request client-side
    // where they have access to their private key
    let signing_keypair = keys::SigningKeypair {
        public_key: signing_key,
        private_key: String::new(), // Empty - we can't access this server-side
    };

    // For now, we'll return an error asking client to generate handshake client-side
    // In a real implementation, the client generates the entire handshake request
    return Err(ApiError::bad_request(
        "Handshake must be generated client-side where private key is available. \
         Use vaultless_core::crypto::handshake::create_handshake_request()",
    ));

    // TODO: Once client-side handshake generation is implemented, this endpoint
    // can be simplified to just validate peer exists and return peer metadata
}

/// Respond to a handshake request
///
/// Verifies the handshake request and creates a session key entry.
/// Returns a signed response to send back to the initiator.
#[utoipa::path(
    post,
    path = "/api/v1/clients/handshake/respond",
    tag = "Session Management",
    security(("bearer_auth" = [])),
    request_body = HandshakeRespondRequest,
    responses(
        (status = 200, description = "Handshake response created", body = HandshakeRespondResponse),
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

    // Get responder's signing key
    let responder = Client::fetch_active_client(
        state.db.as_ref(),
        &state.redis_pool,
        session.client_id,
    )
    .await
    .map_err(ApiError::from)?;

    let signing_key = responder
        .signing_key
        .ok_or_else(|| ApiError::bad_request("Client signing key not found"))?;

    // Convert HandshakeRequestData to handshake::HandshakeRequest
    let request = handshake::HandshakeRequest {
        handshake_id: input.handshake_request.handshake_id.clone(),
        signing_pubkey: input.handshake_request.signing_pubkey.clone(),
        ephemeral_exchange_pubkey: input.handshake_request.ephemeral_exchange_pubkey.clone(),
        timestamp: input.handshake_request.timestamp,
        signature: input.handshake_request.signature.clone(),
    };

    // Similar issue - we need private key to sign response
    // This should also be done client-side
    return Err(ApiError::bad_request(
        "Handshake response must be generated client-side where private key is available. \
         Use vaultless_core::crypto::handshake::respond_to_handshake()",
    ));

    // TODO: Server-side we should only store the session key entries after both
    // clients have completed the handshake
}

/// Store session key after handshake completion
///
/// Internal endpoint called after client-side handshake is complete.
/// Stores the session key metadata in the database.
#[utoipa::path(
    post,
    path = "/api/v1/clients/handshake/complete",
    tag = "Session Management",
    security(("bearer_auth" = [])),
    request_body = HandshakeCompleteRequest,
    responses(
        (status = 200, description = "Session stored successfully", body = HandshakeCompleteResponse),
        (status = 400, description = "Invalid request"),
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

    // Convert to handshake types
    let response = handshake::HandshakeResponse {
        handshake_id: input.handshake_response.handshake_id.clone(),
        signing_pubkey: input.handshake_response.signing_pubkey.clone(),
        ephemeral_exchange_pubkey: input.handshake_response.ephemeral_exchange_pubkey.clone(),
        timestamp: input.handshake_response.timestamp,
        session_id: input.handshake_response.session_id.clone(),
        expires_at: input.handshake_response.expires_at,
        signature: input.handshake_response.signature.clone(),
    };

    // Complete handshake and derive session key
    let result = handshake::complete_handshake(
        &response,
        &input.ephemeral_private_key,
        &input.expected_handshake_id,
    )
    .map_err(|e| ApiError::bad_request(format!("Handshake completion failed: {}", e)))?;

    // Resolve peer client by signing key
    let peer = Client::resolve_client(
        state.db.as_ref(),
        Some(state.redis_pool.clone()),
        Some(&result.peer_signing_pubkey),
        None,
        None,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Peer client not found"))?;

    // Store session key in database
    let session_req = CreateSessionKeyRequest {
        client_id: session.client_id,
        peer_client_id: peer.id,
        application_id: session.application_id,
        session_id: result.session_id.clone(),
        ephemeral_public_key: input.handshake_response.ephemeral_exchange_pubkey.clone(),
        expires_at: result.expires_at,
    };

    SessionKey::create(state.db.as_ref(), session_req)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        client_id = %session.client_id,
        peer_id = %peer.id,
        session_id = %result.session_id,
        expires_at = %result.expires_at,
        "Session established successfully"
    );

    Ok(Json(HandshakeCompleteResponse {
        session_id: result.session_id,
        session_key: result.session_key,
        expires_at: result.expires_at,
        peer_signing_pubkey: result.peer_signing_pubkey,
    }))
}
