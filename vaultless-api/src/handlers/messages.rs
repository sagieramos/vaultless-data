use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use validator::Validate;
use vaultless_core::{ApiKey, CreateMessage, Message, MessageMetadata, UsageMetric};

use crate::{
    middleware::error::ApiError,
    services::{CacheService, cache::message_list_cache_key},
    state::AppState,
};

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    /// Recipient identifier (can be email, user ID, etc.)
    #[validate(length(min = 1, max = 255))]
    pub recipient_id: String,

    /// Base64-encoded ciphertext
    #[validate(length(min = 1))]
    pub ciphertext: String,

    /// Base64-encoded nonce (12 bytes)
    #[validate(length(min = 1, max = 32))]
    pub nonce: String,

    /// Content type (optional)
    pub content_type: Option<String>,

    /// Content size in bytes
    #[validate(range(min = 1))]
    pub content_size_bytes: i32,

    /// Optional TTL in seconds (overrides tier default)
    pub ttl_seconds: Option<i32>,

    /// Max times message can be accessed before auto-deletion
    pub max_access_count: Option<i32>,

    /// Require proof verification before access
    #[serde(default)]
    pub require_proof_verification: bool,
}

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub message_id: String,
    pub recipient_id: String,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ReceiveMessagesResponse {
    pub messages: Vec<MessageResponse>,
    pub total_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    pub ciphertext: String,
    pub nonce: String,
    pub content_type: String,
    pub content_size_bytes: i32,
    pub created_at: String,
    pub expires_at: String,
    pub access_count: i32,
    pub max_access_count: Option<i32>,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// Send encrypted message
/// POST /api/v1/messages/send
pub async fn send_message(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), ApiError> {
    // Validate input
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    tracing::info!(
        api_key_id = %api_key.id,
        recipient_id = %payload.recipient_id,
        "Sending message"
    );

    // Create message in database
    let message = Message::create(
        &state.db,
        CreateMessage {
            recipient_id: payload.recipient_id.clone(),
            ciphertext: payload.ciphertext,
            nonce: payload.nonce,
            content_type: payload.content_type,
            content_size_bytes: payload.content_size_bytes,
            api_key_id: api_key.id,
            ttl_seconds: payload.ttl_seconds,
            max_access_count: payload.max_access_count,
            require_proof_verification: payload.require_proof_verification,
        },
    )
    .await
    .map_err(ApiError::from)?;

    // Record usage metrics
    if let Err(e) =
        UsageMetric::record_message_sent(&state.db, api_key.id, payload.content_size_bytes as i64)
            .await
    {
        tracing::error!("Failed to record usage metrics: {}", e);
        // Don't fail the request, just log the error
    }

    // Invalidate cache for recipient
    let cache = CacheService::new(state.cache.clone(), state.config.cache.default_ttl);
    if let Err(e) = cache
        .delete(&message_list_cache_key(&payload.recipient_id))
        .await
    {
        tracing::warn!("Failed to invalidate cache: {}", e);
        // Don't fail the request
    }

    tracing::info!(
        message_id = %message.id,
        "Message sent successfully"
    );

    Ok((
        StatusCode::CREATED,
        Json(SendMessageResponse {
            message_id: message.id.to_string(),
            recipient_id: message.recipient_id,
            expires_at: message.expires_at.to_rfc3339(),
            created_at: message.created_at.to_rfc3339(),
        }),
    ))
}

/// Receive messages for a recipient
/// GET /api/v1/messages/:recipient_id
pub async fn receive_messages(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    Path(recipient_id): Path<String>,
) -> Result<Json<ReceiveMessagesResponse>, ApiError> {
    tracing::info!(
        api_key_id = %api_key.id,
        recipient_id = %recipient_id,
        "Receiving messages"
    );

    // Try cache first
    let cache = CacheService::new(state.cache.clone(), state.config.cache.default_ttl);
    let cache_key = message_list_cache_key(&recipient_id);

    // Check cache
    if let Ok(Some(cached_messages)) = cache.get::<Vec<MessageResponse>>(&cache_key).await {
        tracing::debug!("Cache hit for recipient {}", recipient_id);

        return Ok(Json(ReceiveMessagesResponse {
            total_count: cached_messages.len(),
            messages: cached_messages,
        }));
    }

    // Cache miss - fetch from database
    tracing::debug!("Cache miss for recipient {}", recipient_id);

    let messages = Message::find_by_recipient(&state.db, &recipient_id, 100)
        .await
        .map_err(ApiError::from)?;

    // Mark messages as accessed
    for msg in &messages {
        if let Err(e) = Message::mark_accessed(&state.db, msg.id).await {
            tracing::error!("Failed to mark message {} as accessed: {}", msg.id, e);
            // Continue processing other messages
        }
    }

    // Record usage metrics
    if let Err(e) = UsageMetric::record_message_received(&state.db, api_key.id).await {
        tracing::error!("Failed to record usage metrics: {}", e);
    }

    // Convert to response format
    let message_responses: Vec<MessageResponse> = messages
        .into_iter()
        .map(|msg| MessageResponse {
            id: msg.id.to_string(),
            ciphertext: msg.ciphertext,
            nonce: msg.nonce,
            content_type: msg.content_type,
            content_size_bytes: msg.content_size_bytes,
            created_at: msg.created_at.to_rfc3339(),
            expires_at: msg.expires_at.to_rfc3339(),
            access_count: msg.access_count,
            max_access_count: msg.max_access_count,
        })
        .collect();

    // Cache the results (cache for 60 seconds)
    let cache_ttl = std::time::Duration::from_secs(60);
    if let Err(e) = cache
        .set_with_ttl(&cache_key, &message_responses, cache_ttl)
        .await
    {
        tracing::warn!("Failed to cache messages: {}", e);
        // Don't fail the request
    }

    tracing::info!(
        count = message_responses.len(),
        "Messages retrieved successfully"
    );

    Ok(Json(ReceiveMessagesResponse {
        total_count: message_responses.len(),
        messages: message_responses,
    }))
}

/// Get message metadata (without ciphertext)
/// GET /api/v1/messages/:message_id/metadata
pub async fn get_message_metadata(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    Path(message_id): Path<String>,
) -> Result<Json<MessageMetadata>, ApiError> {
    let message_uuid = message_id
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid message ID format"))?;

    let message = Message::find_by_id(&state.db, message_uuid)
        .await
        .map_err(ApiError::from)?;

    // Verify the API key has access to this message
    if message.api_key_id != api_key.id {
        return Err(ApiError::forbidden("Access denied to this message"));
    }

    Ok(Json(message.metadata()))
}
