use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;
use chrono;
use vaultless_core::{ApiKey, CreateProof, Message, MessageProof};

use crate::{middleware::error::ApiError, state::AppState};

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct CreateProofRequest {
    #[validate(length(equal = 64))] // SHA-256 hex = 64 chars
    pub content_hash: String,

    #[validate(length(min = 1))]
    pub signature: String,

    #[validate(length(min = 1))]
    pub public_key: String,

    pub algorithm: Option<String>,
    pub hash_algorithm: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateProofResponse {
    pub proof_id: String,
    pub message_id: String,
    pub content_hash: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct VerifyProofRequest {
    #[validate(length(equal = 64))]
    pub content_hash: String,

    #[validate(length(min = 1))]
    pub signature: String,

    #[validate(length(min = 1))]
    pub public_key: String,

    /// Optional: Original plaintext data (for verification)
    pub plaintext_data: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VerifyProofResponse {
    pub is_valid: bool,
    pub message_id: String,
    pub proof_id: String,
    pub verified_at: String,
    pub verification_count: i32,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// Create a cryptographic proof for a message
/// POST /api/v1/messages/:message_id/proof
pub async fn create_proof(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    Path(message_id): Path<String>,
    Json(payload): Json<CreateProofRequest>,
) -> Result<(StatusCode, Json<CreateProofResponse>), ApiError> {
    // Validate input
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let message_uuid: Uuid = message_id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid message ID format"))?;

    tracing::info!(
        api_key_id = %api_key.id,
        message_id = %message_uuid,
        "Creating proof"
    );

    // Verify message exists and belongs to this API key
    let message = Message::find_by_id(&state.db, message_uuid)
        .await
        .map_err(ApiError::from)?;

    if message.api_key_id != api_key.id {
        return Err(ApiError::forbidden("Access denied to this message"));
    }

    // Create proof
    let proof = MessageProof::create(
        &state.db,
        CreateProof {
            message_id: message_uuid,
            content_hash: payload.content_hash.clone(),
            signature: payload.signature,
            public_key: payload.public_key,
            algorithm: payload.algorithm,
            hash_algorithm: payload.hash_algorithm,
            proof_metadata: None,
        },
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        proof_id = %proof.id,
        "Proof created successfully"
    );

    Ok((
        StatusCode::CREATED,
        Json(CreateProofResponse {
            proof_id: proof.id.to_string(),
            message_id: proof.message_id.to_string(),
            content_hash: proof.content_hash,
            created_at: proof.created_at.to_rfc3339(),
        }),
    ))
}

/// Verify a message proof cryptographically
/// POST /api/v1/messages/:message_id/verify
pub async fn verify_message_proof(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    Path(message_id): Path<String>,
    Json(payload): Json<VerifyProofRequest>,
) -> Result<Json<VerifyProofResponse>, ApiError> {
    // Validate input
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let message_uuid: Uuid = message_id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid message ID format"))?;

    tracing::info!(
        api_key_id = %api_key.id,
        message_id = %message_uuid,
        "Verifying proof"
    );

    // Find the proof
    let proof = MessageProof::find_by_message_id(&state.db, message_uuid)
        .await
        .map_err(ApiError::from)?;

    // Validate proof data matches
    if proof.content_hash != payload.content_hash {
        return Err(ApiError::bad_request("Content hash mismatch"));
    }

    if proof.signature != payload.signature {
        return Err(ApiError::bad_request("Signature mismatch"));
    }

    if proof.public_key != payload.public_key {
        return Err(ApiError::bad_request("Public key mismatch"));
    }

    // Perform cryptographic verification
    // If plaintext_data provided, verify hash
    if let Some(plaintext_b64) = &payload.plaintext_data {
        let plaintext_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, plaintext_b64)
                .map_err(|_| ApiError::bad_request("Invalid base64 plaintext data"))?;

        // Verify content hash
        let computed_hash = vaultless_core::crypto::hash_content(&plaintext_bytes);
        if computed_hash != proof.content_hash {
            return Err(ApiError::bad_request(
                "Content hash does not match plaintext data",
            ));
        }

        // Verify Ed25519 signature
        vaultless_core::crypto::verify_signature(
            &plaintext_bytes,
            &proof.signature,
            &proof.public_key,
        )
        .map_err(|_| ApiError::forbidden("Signature verification failed"))?;

        tracing::info!("Cryptographic verification successful");
    }

    // Mark proof as verified
    let updated_proof = MessageProof::mark_verified(&state.db, proof.id)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        proof_id = %updated_proof.id,
        verification_count = updated_proof.verification_count,
        "Proof verified successfully"
    );

    Ok(Json(VerifyProofResponse {
        is_valid: true,
        message_id: updated_proof.message_id.to_string(),
        proof_id: updated_proof.id.to_string(),
        verified_at: updated_proof
            .verified_at
            .unwrap_or(chrono::Utc::now())
            .to_rfc3339(),
        verification_count: updated_proof.verification_count,
    }))
}

/// Get proof details for a message
/// GET /api/v1/messages/:message_id/proof
pub async fn get_message_proof(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    Path(message_id): Path<String>,
) -> Result<Json<MessageProof>, ApiError> {
    let message_uuid: Uuid = message_id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid message ID format"))?;

    // Verify message belongs to this API key
    let message = Message::find_by_id(&state.db, message_uuid)
        .await
        .map_err(ApiError::from)?;

    if message.api_key_id != api_key.id {
        return Err(ApiError::forbidden("Access denied to this message"));
    }

    // Get proof
    let proof = MessageProof::find_by_message_id(&state.db, message_uuid)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(proof))
}

/// Search proofs by content hash (public verification)
/// GET /api/v1/proofs/by-hash/:content_hash
pub async fn find_proofs_by_hash(
    State(state): State<AppState>,
    Path(content_hash): Path<String>,
) -> Result<Json<Vec<MessageProof>>, ApiError> {
    // Validate hash format
    if content_hash.len() != 64 {
        return Err(ApiError::bad_request(
            "Invalid content hash format. Expected 64-character SHA-256 hex",
        ));
    }

    tracing::info!(content_hash = %content_hash, "Searching proofs by hash");

    let proofs = MessageProof::find_by_content_hash(&state.db, &content_hash)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(proofs))
}
