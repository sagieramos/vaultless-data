// vaultless-api/src/handlers/instant_message.rs
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::middleware::error::ApiError;
use crate::{AppState, middleware::client::AuthenticatedClient};
use vaultless_core::models::instant_message::{Message, ReadReceipt};

// =============================================================================
// Request Types
// =============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    /// Recipient client ID
    pub recipient_client_id: Uuid,

    /// Encrypted message content (base64 or hex)
    #[validate(length(min = 1, max = 1048576))] // 1MB max
    pub ciphertext: String,

    /// Nonce for encryption
    pub nonce: Uuid,

    /// Size of the original content in bytes
    #[validate(range(min = 1, max = 10485760))] // 10MB max
    pub content_size_bytes: i32,

    /// API key ID for rate limiting and quotas
    pub api_key_id: Uuid,

    /// Ed25519/P-256 signature of envelope
    #[validate(length(min = 64, max = 256))]
    pub signature: String,

    /// Public key used for signature verification
    #[validate(length(min = 32, max = 256))]
    pub envelope_public_key: String,

    /// Whether to require proof verification
    pub require_proof_verification: bool,
}

#[derive(Debug, Deserialize)]
pub struct FetchMessagesQuery {
    /// Optional limit (defaults to service max)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct MarkReadRequest {
    /// Message ID to mark as read
    pub message_id: Uuid,
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
    pub messages: Vec<MessagePublic>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct MessagePublic {
    pub id: Uuid,
    pub ciphertext: String,
    pub nonce: Uuid,
    pub content_type: Option<String>,
    pub content_size_bytes: i32,
    pub created_at: String,
    pub expires_at: String,
    pub sender_client_id: Uuid,
    pub recipient_client_id: Uuid,
    pub is_group_message: bool,
    pub signature: String,
    pub envelope_public_key: String,
}

impl From<Message> for MessagePublic {
    fn from(msg: Message) -> Self {
        Self {
            id: msg.id,
            ciphertext: msg.ciphertext,
            nonce: msg.nonce,
            content_type: msg.content_type,
            content_size_bytes: msg.content_size_bytes,
            created_at: msg.created_at.to_rfc3339(),
            expires_at: msg.expires_at.to_rfc3339(),
            sender_client_id: msg.sender_client_id,
            recipient_client_id: msg.recipient_client_id,
            is_group_message: msg.is_group_message,
            signature: msg.signature,
            envelope_public_key: msg.envelope_public_key,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MarkReadResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ReadReceiptsResponse {
    pub success: bool,
    pub receipts: Vec<ReadReceiptPublic>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ReadReceiptPublic {
    pub id: Uuid,
    pub message_id: Uuid,
    pub client_id: Uuid,
    pub read_at: String,
}

impl From<ReadReceipt> for ReadReceiptPublic {
    fn from(receipt: ReadReceipt) -> Self {
        Self {
            id: receipt.id,
            message_id: receipt.message_id,
            client_id: receipt.client_id,
            read_at: receipt.read_at.to_rfc3339(),
        }
    }
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
/// POST /api/messages/send
#[axum::debug_handler]
pub async fn send_message(
    State(state): State<AppState>,
    AuthenticatedClient(client): AuthenticatedClient,
    Json(input): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    // Validate input
    input
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()).with_code("VALIDATION_ERROR"))?;

    tracing::info!(
        sender = %client.id,
        recipient = %input.recipient_client_id,
        size = input.content_size_bytes,
        "Sending instant message"
    );

    // Send message through InstantMessage service
    let message_id = state
        .instant_message
        .send_instant_message(
            client.id,
            input.recipient_client_id,
            input.ciphertext,
            input.nonce,
            input.content_size_bytes,
            input.api_key_id,
            input.signature,
            input.envelope_public_key,
            input.require_proof_verification,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                sender = %client.id,
                error = %e,
                "Failed to send message"
            );
            ApiError::from(e)
        })?;

    tracing::info!(
        message_id = %message_id,
        sender = %client.id,
        "Message sent successfully"
    );

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
    AuthenticatedClient(client): AuthenticatedClient,
    Query(_query): Query<FetchMessagesQuery>,
) -> Result<Json<FetchMessagesResponse>, ApiError> {
    tracing::debug!(recipient = %client.id, "Fetching inbox");

    // Fetch messages from InstantMessage service
    let messages = state
        .instant_message
        .fetch_messages_for_recipient(client.id)
        .await
        .map_err(|e| {
            tracing::error!(
                recipient = %client.id,
                error = %e,
                "Failed to fetch messages"
            );
            ApiError::from(e)
        })?;

    let count = messages.len();
    let public_messages: Vec<MessagePublic> = messages.into_iter().map(Into::into).collect();

    tracing::info!(
        recipient = %client.id,
        count,
        "Fetched messages successfully"
    );

    Ok(Json(FetchMessagesResponse {
        success: true,
        messages: public_messages,
        count,
    }))
}

/// Mark a message as read
/// POST /api/messages/{message_id}/read
#[axum::debug_handler]
pub async fn mark_message_read(
    State(state): State<AppState>,
    AuthenticatedClient(client): AuthenticatedClient,
    Path(message_id): Path<Uuid>,
) -> Result<Json<MarkReadResponse>, ApiError> {
    tracing::debug!(
        reader = %client.id,
        message_id = %message_id,
        "Marking message as read"
    );

    state
        .instant_message
        .mark_read_instant_message(client.id, message_id)
        .await
        .map_err(|e| {
            tracing::error!(
                reader = %client.id,
                message_id = %message_id,
                error = %e,
                "Failed to mark message as read"
            );
            ApiError::from(e)
        })?;

    tracing::info!(
        reader = %client.id,
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
    AuthenticatedClient(client): AuthenticatedClient,
    Path(message_id): Path<Uuid>,
) -> Result<Json<ReadReceiptsResponse>, ApiError> {
    tracing::debug!(
        requester = %client.id,
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
    let public_receipts: Vec<ReadReceiptPublic> = receipts.into_iter().map(Into::into).collect();

    Ok(Json(ReadReceiptsResponse {
        success: true,
        receipts: public_receipts,
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
