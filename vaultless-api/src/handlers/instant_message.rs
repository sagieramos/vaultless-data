// vaultless-api/src/handlers/instant_message.rs
use crate::AppState;
use crate::middleware::client::AuthenticatedClient;
use crate::middleware::error::ApiError;
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;
use vaultless_core::Client;
use vaultless_core::models::instant_message::{Message, ReadReceipt};

// =============================================================================
// Request Types
// =============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    pub recipient_identifier: Option<String>,
    pub recipient_pubkey: Option<String>,
    /// Encrypted message content (base64 or hex)
    #[validate(length(min = 1, max = 1048576))] // 1MB max
    pub ciphertext: String,

    /// Nonce for encryption
    pub nonce: Uuid,

    /// Ed25519/P-256 signature of envelope
    #[validate(length(min = 64, max = 256))]
    pub signature: Option<String>,

    /// Whether to require proof verification
    pub require_proof_verification: bool,
}

#[derive(Debug, Deserialize)]
pub struct FetchMessagesQuery {
    /// Optional limit (defaults to service max)
    pub limit: Option<usize>,
}

// =============================================================================
// Response Types
// =============================================================================

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub success: bool,
    pub message_id: Uuid,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct FetchMessagesResponse {
    pub success: bool,
    pub messages: Vec<Message>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct MarkReadResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ReadReceiptsResponse {
    pub success: bool,
    pub receipts: Vec<ReadReceipt>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct HealthStatusResponse {
    pub success: bool,
    pub status: vaultless_core::models::instant_message::HealthStatus,
}

// =============================================================================
// Message Handlers
// =============================================================================

/// Send an instant message (P2P)
/// POST /api/messages/send#
#[axum::debug_handler]
pub async fn send_message(
    State(state): State<AppState>,
    Extension(sender): Extension<AuthenticatedClient>,
    Json(input): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    // --- 1. Compute content size server-side ---
    let content_size_bytes = input.ciphertext.as_bytes().len() as i32;
    
    // Validate input
    input
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()).with_code("VALIDATION_ERROR"))?;

    // --- 2. Resolve recipient ---
    let recipient = Client::resolve_client(
        &*state.db,
        Some(state.redis_pool.clone()),
        input.recipient_pubkey.as_deref(), // public key takes priority
        input.recipient_identifier.as_deref(),
        None,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Recipient client not found"))?;

    let sender_pubkey = recipient.public_key.ok_or_else(|| {
        tracing::error!("Sender public key not found in database");
        ApiError::bad_request("Sender public key not found")
    })?;

    tracing::info!(
        sender = %sender.0.id,
        recipient = %recipient.id,
        size = content_size_bytes,
        "Sending instant message"
    );

    // --- 3. Send message ---
    let message_id = state
        .instant_message
        .send_instant_message(
            sender.0.id,
            recipient.id,
            input.ciphertext.clone(),
            input.nonce,
            content_size_bytes,
            input.nonce,
            input.signature.clone(),
            sender_pubkey,
            input.require_proof_verification,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                sender = %sender.0.id,
                error = %e,
                "Failed to send message"
            );
            ApiError::from(e)
        })?;

    Ok(Json(SendMessageResponse {
        success: true,
        message_id,
        created_at: chrono::Utc::now().to_rfc3339(),
    }))
}

/// Fetch messages for current user (inbox)
/// GET /api/messages/inbox
#[axum::debug_handler]
pub async fn fetch_inbox(
    State(state): State<AppState>,
    Extension(client): Extension<AuthenticatedClient>,
    Query(_query): Query<FetchMessagesQuery>,
) -> Result<Json<FetchMessagesResponse>, ApiError> {
    tracing::debug!(recipient = %client.0.id, "Fetching inbox");

    // Fetch messages from InstantMessage service
    let messages = state
        .instant_message
        .fetch_messages_for_recipient(client.0.id)
        .await
        .map_err(|e| {
            tracing::error!(
                recipient = %client.0.id,
                error = %e,
                "Failed to fetch messages"
            );
            ApiError::from(e)
        })?;

    let count = messages.len();

    tracing::info!(
        recipient = %client.0.id,
        count,
        "Fetched messages successfully"
    );

    Ok(Json(FetchMessagesResponse {
        success: true,
        messages,
        count,
    }))
}

/// Mark a message as read
/// POST /api/messages/{message_id}/read
#[axum::debug_handler]
pub async fn mark_message_read(
    State(state): State<AppState>,
    Extension(client): Extension<AuthenticatedClient>,
    Path(message_id): Path<Uuid>,
) -> Result<Json<MarkReadResponse>, ApiError> {
    tracing::debug!(
        reader = %client.0.id,
        message_id = %message_id,
        "Marking message as read"
    );

    state
        .instant_message
        .mark_read_instant_message(client.0.id, message_id)
        .await
        .map_err(|e| {
            tracing::error!(
                reader = %client.0.id,
                message_id = %message_id,
                error = %e,
                "Failed to mark message as read"
            );
            ApiError::from(e)
        })?;

    tracing::info!(
        reader = %client.0.id,
        message_id = %message_id,
        "Message marked as read"
    );

    Ok(Json(MarkReadResponse {
        success: true,
        message: "Message marked as read".to_string(),
    }))
}

/// Get read receipts for a message
/// GET /api/messages/{message_id}/receipts
#[axum::debug_handler]
pub async fn get_read_receipts(
    State(state): State<AppState>,
    Extension(client): Extension<AuthenticatedClient>,
    Path(message_id): Path<Uuid>,
) -> Result<Json<ReadReceiptsResponse>, ApiError> {
    tracing::debug!(
        requester = %client.0.id,
        message_id = %message_id,
        "Fetching read receipts"
    );

    let receipts = state
        .instant_message
        .fetch_read_receipts(message_id)
        .await
        .map_err(|e| {
            tracing::error!(
                message_id = %message_id,
                error = %e,
                "Failed to fetch read receipts"
            );
            ApiError::from(e)
        })?;

    let count = receipts.len();

    Ok(Json(ReadReceiptsResponse {
        success: true,
        receipts,
        count,
    }))
}

/// Health check for InstantMessage service
/// GET /api/messages/health
#[axum::debug_handler]
pub async fn message_health_check(State(state): State<AppState>) -> Json<HealthStatusResponse> {
    let status = state.instant_message.get_health_status();

    Json(HealthStatusResponse {
        success: true,
        status,
    })
}
