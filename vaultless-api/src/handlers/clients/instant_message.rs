use crate::{
    middleware::{
        application::ApplicationKeyViewExt, client::SessionDataClientExt, error::ApiError,
    },
    services::real_time_message::InstantMessageExt,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;
use chrono;
use vaultless_core::Client;
use vaultless_core::models::message::dto::{HealthStatus, Message, ReadReceipt};

// =============================================================================
// Request Types (UPDATED)
// =============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    pub recipient_identifier: Option<String>,
    pub recipient_pubkey: Option<String>,

    /// Encrypted message content (base64 or hex)
    #[validate(length(min = 1, max = 10485760))] // UPDATED: 10MB max (was 1MB)
    pub ciphertext: String,

    /// Nonce for encryption
    pub nonce: Uuid,

    /// Ed25519/P-256 signature of envelope (NOW REQUIRED if verification enabled)
    #[validate(length(min = 64, max = 256))]
    pub signature: Option<String>,

    /// Whether to require proof verification (defaults to true)
    #[serde(default = "default_require_verification")]
    pub require_proof_verification: bool,
}

fn default_require_verification() -> bool {
    true // Default to requiring verification for security
}

// =============================================================================
// Response Types (UPDATED)
// =============================================================================

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub success: bool,
    pub message_id: Uuid,
    pub created_at: String,
    /// Whether recipient is online (WebSocket connected)
    pub recipient_online: bool,
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
    pub read_at: String,
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
    pub status: HealthStatus,
    pub websocket_connections: usize,
}

// =============================================================================
// UPDATED Message Handlers
// =============================================================================

/// Send an instant message (P2P) - UPDATED WITH ALL FIXES
/// POST /api/messages/send
pub async fn send_message(
    State(state): State<AppState>,
    SessionDataClientExt(sender): SessionDataClientExt,
    ApplicationKeyViewExt(app): ApplicationKeyViewExt,
    Json(input): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    // --- 1. Validate input ---
    input
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()).with_code("VALIDATION_ERROR"))?;

    // --- 2. Compute content size server-side ---
    let content_size_bytes = input.ciphertext.len() as i64; 

    // --- 3. Verify signature is provided if verification required ---
    if input.require_proof_verification && input.signature.is_none() {
        return Err(
            ApiError::bad_request("Signature required when proof verification is enabled")
                .with_code("SIGNATURE_REQUIRED"),
        );
    }

    // --- 4. Resolve recipient ---
    let recipient = Client::resolve_client(
        state.db.as_ref(),
        Some(state.redis_pool.clone()),
        input.recipient_pubkey.as_deref(),
        input.recipient_identifier.as_deref(),
        None,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("Recipient client not found"))?;

    // --- 5. Get sender's public key for envelope verification ---
    let sender_pubkey = sender.pubkey.clone().ok_or_else(|| {
        tracing::error!(
            sender_id = %sender.client_id,
            "Sender public key not found in session"
        );
        ApiError::bad_request("Sender public key not found")
    })?;

    tracing::info!(
        sender = %sender.client_id,
        recipient = %recipient.id,
        size = content_size_bytes,
        requires_verification = input.require_proof_verification,
        "Sending instant message"
    );

    // --- 6. Send message (now with signature verification at send time) ---
    let message_id = state
        .instant_message
        .send_instant_message(
            sender.client_id,
            recipient.id,
            input.ciphertext.clone(),
            input.nonce,
            content_size_bytes,
            app.sk_id,
            input.signature.clone(),
            sender_pubkey,
            input.require_proof_verification,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                sender = %sender.client_id,
                recipient = %recipient.id,
                error = %e,
                "Failed to send message"
            );

            // Map specific errors to appropriate HTTP status codes
            ApiError::from(e)
        })?;

    // --- 7. Send WebSocket notification to recipient (if connected) ---
    let recipient_online = state.ws_manager.is_connected(&recipient.id);

    if recipient_online {
        state
            .instant_message
            .notify_message_sent(
                &state.ws_manager,
                message_id,
                sender.client_id,
                recipient.id,
            )
            .await;
    }

    tracing::info!(
        sender = %sender.client_id,
        recipient = %recipient.id,
        message_id = %message_id,
        recipient_online = recipient_online,
        "Message sent successfully"
    );

    Ok(Json(SendMessageResponse {
        success: true,
        message_id,
        created_at: chrono::Utc::now().to_rfc3339(),
        recipient_online,
    }))
}

/// Fetch messages for current user (inbox) - UPDATED
/// GET /api/messages/inbox
pub async fn fetch_inbox(
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
) -> Result<Json<FetchMessagesResponse>, ApiError> {
    tracing::debug!(
        recipient = %client_info.client_id,
        "Fetching inbox"
    );

    // Fetch messages (now with atomic delivery counting & signature verification)
    let messages = state
        .instant_message
        .fetch_messages_for_recipient(client_info.client_id)
        .await
        .map_err(|e| {
            tracing::error!(
                recipient = %client_info.client_id,
                error = %e,
                "Failed to fetch messages"
            );
            ApiError::from(e)
        })?;

    let count = messages.len();

    tracing::info!(
        recipient = %client_info.client_id,
        count,
        "Fetched messages successfully"
    );

    Ok(Json(FetchMessagesResponse {
        success: true,
        messages,
        count,
    }))
}

/// Mark a message as read - UPDATED
/// POST /api/messages/{message_id}/read
pub async fn mark_message_read(
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
    Path(message_id): Path<Uuid>,
) -> Result<Json<MarkReadResponse>, ApiError> {
    tracing::debug!(
        reader = %client_info.client_id,
        message_id = %message_id,
        "Marking message as read"
    );

    // Mark as read (now handles pending reads for Redis-only messages)
    state
        .instant_message
        .mark_read_instant_message(client_info.client_id, message_id)
        .await
        .map_err(|e| {
            tracing::error!(
                reader = %client_info.client_id,
                message_id = %message_id,
                error = %e,
                "Failed to mark message as read"
            );
            ApiError::from(e)
        })?;

    let read_at = chrono::Utc::now();

    tracing::info!(
        reader = %client_info.client_id,
        message_id = %message_id,
        "Message marked as read"
    );

    Ok(Json(MarkReadResponse {
        success: true,
        message: "Message marked as read".to_string(),
        read_at: read_at.to_rfc3339(),
    }))
}

/// Get read receipts for a message - UPDATED
/// GET /api/messages/{message_id}/receipts
pub async fn get_read_receipts(
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
    ApplicationKeyViewExt(_): ApplicationKeyViewExt,
    Path(message_id): Path<Uuid>,
) -> Result<Json<ReadReceiptsResponse>, ApiError> {
    tracing::debug!(
        requester = %client_info.client_id,
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

/// Health check for InstantMessage service - UPDATED
/// GET /api/messages/health
#[axum::debug_handler]
pub async fn message_health_check(State(state): State<AppState>) -> Json<HealthStatusResponse> {
    let status = state.instant_message.get_health_status();
    let ws_connections = state.ws_manager.connection_count();

    Json(HealthStatusResponse {
        success: true,
        status,
        websocket_connections: ws_connections,
    })
}
