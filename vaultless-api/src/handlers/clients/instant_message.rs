use crate::{
    middleware::{
        application::AuthCacheKey, application::ApplicationKeyViewExt,
        client::SessionDataClientExt, error::ApiError,
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
use utoipa::ToSchema;
use vaultless_core::Client;
use vaultless_core::models::message::dto::{HealthStatus, MessageResponse, ReadReceipt};

// =============================================================================
// Request Types (UPDATED)
// =============================================================================

#[derive(Debug, Deserialize, Validate, ToSchema)]
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

    /// Optional session ID for session-based encryption
    pub session_id: Option<String>,

    /// Encryption algorithm used (defaults to "xchacha20-poly1305")
    pub encryption_algorithm: Option<String>,

    /// Algorithm version (defaults to 1)
    pub algorithm_version: Option<i16>,
}

fn default_require_verification() -> bool {
    true // Default to requiring verification for security
}

// =============================================================================
// Response Types (UPDATED)
// =============================================================================

#[derive(Debug, Serialize, ToSchema)]
pub struct SendMessageResponse {
    pub success: bool,
    pub message_id: Uuid,
    pub created_at: String,
    /// Whether recipient is online (WebSocket connected)
    pub recipient_online: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FetchMessagesResponse {
    pub success: bool,
    pub messages: Vec<MessageResponse>,
    pub count: usize,
}

/// Grouped inbox entry - last message from a sender
#[derive(Debug, Serialize, ToSchema)]
pub struct InboxEntry {
    /// Sender's public key
    pub sender_pubkey: String,
    /// The last message from this sender
    pub last_message: MessageResponse,
    /// Total unread count from this sender (optional, for future use)
    pub unread_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupedInboxResponse {
    pub success: bool,
    /// Messages grouped by sender public key, with only the last message per sender
    pub inbox: Vec<InboxEntry>,
    /// Total number of unique senders
    pub sender_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MarkReadResponse {
    pub success: bool,
    pub message: String,
    pub read_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReadReceiptsResponse {
    pub success: bool,
    #[schema(value_type = Vec<Object>)]
    pub receipts: Vec<ReadReceipt>,
    pub count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthStatusResponse {
    pub success: bool,
    #[schema(value_type = Object)]
    pub status: HealthStatus,
    pub websocket_connections: usize,
}

/// Pagination query parameters for fetching messages by sender
#[derive(Debug, Deserialize, ToSchema)]
pub struct PaginationQuery {
    /// Number of messages to skip (default: 0)
    #[serde(default)]
    pub offset: usize,
    /// Maximum number of messages to return (default: 20, max: 100)
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Response for paginated messages from a specific sender
#[derive(Debug, Serialize, ToSchema)]
pub struct SenderMessagesResponse {
    pub success: bool,
    /// Sender's public key
    pub sender_pubkey: String,
    /// Messages from this sender (sorted by created_at descending)
    pub messages: Vec<MessageResponse>,
    /// Number of messages returned
    pub count: usize,
    /// Total messages available from this sender
    pub total: usize,
    /// Current offset
    pub offset: usize,
    /// Whether there are more messages
    pub has_more: bool,
}

// =============================================================================
// UPDATED Message Handlers
// =============================================================================

/// Send an instant message (P2P) - UPDATED WITH ALL FIXES
/// POST /api/messages/send
#[utoipa::path(
    post,
    path = "/api/messages/send",
    tag = "Instant Messaging",
    security(("bearer_auth" = [])),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "Message sent successfully", body = SendMessageResponse),
        (status = 400, description = "Invalid request"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Recipient not found")
    )
)]
pub async fn send_message(
    State(state): State<AppState>,
    SessionDataClientExt(sender): SessionDataClientExt,
    ApplicationKeyViewExt(app): ApplicationKeyViewExt,
    AuthCacheKey(auth_cache_key): AuthCacheKey,
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

    // --- 6. Send message using v2 with auth cache key ---
    // Use app.app_id (application_id) instead of sk_id for metrics tracking
    // Application ID is stable across API key rotations
    let message_id = state
        .instant_message
        .send_instant_message_v2_with_auth_key(
            sender.client_id,
            recipient.id,
            input.ciphertext.clone(),
            input.nonce,
            content_size_bytes,
            app.app_id,
            input.session_id.clone().unwrap_or_else(|| "default".to_string()),
            input.signature.clone(),
            sender_pubkey,
            input.require_proof_verification,
            input.encryption_algorithm.clone(),
            input.algorithm_version,
            auth_cache_key,
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

/// Fetch messages for current user (inbox) - grouped by sender
/// GET /api/messages/inbox
///
/// Returns messages grouped by sender public key, with only the last message
/// from each sender. Messages are sorted by created_at in descending order.
/// This is a read-only peek operation with no side effects.
#[utoipa::path(
    get,
    path = "/api/messages/inbox",
    tag = "Instant Messaging",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Inbox fetched successfully", body = GroupedInboxResponse),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn fetch_inbox(
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
) -> Result<Json<GroupedInboxResponse>, ApiError> {
    tracing::debug!(
        recipient = %client_info.client_id,
        "Fetching grouped inbox"
    );

    // Peek inbox (read-only, no side effects like delivery counting or deletion)
    let grouped_inbox = state
        .instant_message
        .peek_inbox_grouped(client_info.client_id)
        .await
        .map_err(|e| {
            tracing::error!(
                recipient = %client_info.client_id,
                error = %e,
                "Failed to peek inbox"
            );
            ApiError::from(e)
        })?;

    // Convert core InboxEntry to API InboxEntry with MessageResponse
    let inbox: Vec<InboxEntry> = grouped_inbox
        .entries
        .into_iter()
        .map(|entry| InboxEntry {
            sender_pubkey: entry.sender_pubkey,
            last_message: MessageResponse::from(entry.last_message),
            unread_count: entry.message_count,
        })
        .collect();

    let sender_count = grouped_inbox.sender_count;

    tracing::info!(
        recipient = %client_info.client_id,
        sender_count,
        total_messages = grouped_inbox.total_messages,
        "Fetched grouped inbox successfully"
    );

    Ok(Json(GroupedInboxResponse {
        success: true,
        inbox,
        sender_count,
    }))
}

/// Fetch messages from a specific sender with pagination
/// GET /api/messages/sender/{sender_pubkey}
///
/// Returns paginated messages from a specific sender, sorted by created_at descending.
/// This is a read-only peek operation with no side effects.
#[utoipa::path(
    get,
    path = "/api/messages/sender/{sender_pubkey}",
    tag = "Instant Messaging",
    security(("bearer_auth" = [])),
    params(
        ("sender_pubkey" = String, Path, description = "Sender's public key"),
        ("offset" = Option<usize>, Query, description = "Number of messages to skip (default: 0)"),
        ("limit" = Option<usize>, Query, description = "Maximum messages to return (default: 20, max: 100)")
    ),
    responses(
        (status = 200, description = "Messages fetched successfully", body = SenderMessagesResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Sender not found")
    )
)]
pub async fn fetch_messages_by_sender(
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
    Path(sender_pubkey): Path<String>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<Json<SenderMessagesResponse>, ApiError> {
    // Validate and cap limit
    let limit = pagination.limit.min(100);
    let offset = pagination.offset;

    tracing::debug!(
        recipient = %client_info.client_id,
        sender_pubkey = %sender_pubkey,
        offset,
        limit,
        "Fetching messages by sender"
    );

    // Fetch paginated messages from this sender
    let sender_messages = state
        .instant_message
        .fetch_messages_by_sender(client_info.client_id, &sender_pubkey, offset, limit)
        .await
        .map_err(|e| {
            tracing::error!(
                recipient = %client_info.client_id,
                sender_pubkey = %sender_pubkey,
                error = %e,
                "Failed to fetch messages by sender"
            );
            ApiError::from(e)
        })?;

    let count = sender_messages.messages.len();

    tracing::info!(
        recipient = %client_info.client_id,
        sender_pubkey = %sender_pubkey,
        count,
        total = sender_messages.total,
        has_more = sender_messages.has_more,
        "Fetched messages by sender successfully"
    );

    Ok(Json(SenderMessagesResponse {
        success: true,
        sender_pubkey: sender_messages.sender_pubkey,
        messages: MessageResponse::from_vec(sender_messages.messages),
        count,
        total: sender_messages.total,
        offset: sender_messages.offset,
        has_more: sender_messages.has_more,
    }))
}

/// Mark a message as read - UPDATED
/// POST /api/messages/{message_id}/read
#[utoipa::path(
    post,
    path = "/api/messages/{message_id}/read",
    tag = "Instant Messaging",
    security(("bearer_auth" = [])),
    params(
        ("message_id" = Uuid, Path, description = "Message ID to mark as read")
    ),
    responses(
        (status = 200, description = "Message marked as read", body = MarkReadResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Message not found")
    )
)]
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
#[utoipa::path(
    get,
    path = "/api/messages/{message_id}/receipts",
    tag = "Instant Messaging",
    security(("bearer_auth" = [])),
    params(
        ("message_id" = Uuid, Path, description = "Message ID to get receipts for")
    ),
    responses(
        (status = 200, description = "Read receipts fetched successfully", body = ReadReceiptsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Message not found")
    )
)]
pub async fn get_read_receipts(
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
    ApplicationKeyViewExt(_app): ApplicationKeyViewExt,
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
#[utoipa::path(
    get,
    path = "/api/messages/health",
    tag = "Instant Messaging",
    responses(
        (status = 200, description = "Message service health status", body = HealthStatusResponse)
    )
)]
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
