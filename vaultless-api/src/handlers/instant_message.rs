use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info};
use uuid::Uuid;

use vaultless_core::error::{Result, VaultlessError};
use vaultless_core::{
    InstantMessage, Message, P2PFile, ReadReceipt, HealthStatus,
};
use vaultless_core::crypto::verify_signature; 

// =============================================================================
// Request/Response DTOs
// =============================================================================

/// Request to send a P2P instant message.
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub recipient_client_id: Uuid,
    pub ciphertext: String,
    pub nonce: Uuid,
    pub content_size_bytes: i32,
    pub signature: String,
    pub envelope_public_key: String,
    pub require_proof_verification: bool,
}

/// Response for sent message (just the ID).
#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub message_id: Uuid,
}

/// Request to mark a message as read.
#[derive(Debug, Deserialize)]
pub struct MarkReadRequest {
    pub reader_client_id: Uuid,
}

/// Response for fetched messages.
#[derive(Debug, Serialize)]
pub struct FetchMessagesResponse {
    pub messages: Vec<Message>,
}

/// Response for read receipts.
#[derive(Debug, Serialize)]
pub struct FetchReceiptsResponse {
    pub receipts: Vec<ReadReceipt>,
}

/// Query params for health (optional filters, but basic for now).
#[derive(Debug, Deserialize)]
pub struct HealthQuery {
    // Placeholder for future expansions (e.g., ?include=channels)
}

/// =============================================================================
// Handlers
// =============================================================================

/// App state holding the shared `InstantMessage` instance.
#[derive(Clone)]
pub struct AppState {
    pub instant_msg: Arc<InstantMessage>,
}

/// POST /v1/messages/send
/// Sends a P2P instant message.
/// Extracts api_key_id from header (e.g., X-API-Key-Id) and sender_client_id from another header or JWT.
/// For simplicity, assume sender_client_id and api_key_id are extracted via middleware or headers.
pub async fn handler_send_message(
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
    // Placeholder extracts - in production, use Extension or Header for auth
    // sender_client_id: Extension<Uuid>, // From JWT or auth middleware
    // api_key_id: Header<Uuid>, // From X-API-Key-Id
) -> Result<Json<SendMessageResponse>, impl IntoResponse> {
    // TODO: Extract sender_client_id and api_key_id from auth context
    let sender_client_id = Uuid::parse_str(&std::env::var("TEST_SENDER_ID").unwrap_or_default()).unwrap_or_default(); // Placeholder
    let api_key_id = Uuid::parse_str(&std::env::var("TEST_API_KEY_ID").unwrap_or_default()).unwrap_or_default(); // Placeholder

    let msg_id = state
        .instant_msg
        .send_instant_message(
            sender_client_id,
            req.recipient_client_id,
            req.ciphertext,
            req.nonce,
            req.content_size_bytes,
            api_key_id,
            req.signature,
            req.envelope_public_key,
            req.require_proof_verification,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Send message failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    info!(message_id = %msg_id, "API: Message sent");
    Ok(Json(SendMessageResponse { message_id }))
}

/// GET /v1/messages/:recipient_client_id
/// Fetches undelivered messages for the recipient.
pub async fn handler_fetch_messages(
    State(state): State<AppState>,
    Path(recipient_client_id): Path<Uuid>,
) -> Result<Json<FetchMessagesResponse>, impl IntoResponse> {
    let messages = state
        .instant_msg
        .fetch_messages_for_recipient(recipient_client_id)
        .await
        .map_err(|e| {
            error!(recipient = %recipient_client_id, error = %e, "Fetch messages failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    info!(recipient = %recipient_client_id, count = messages.len(), "API: Messages fetched");
    Ok(Json(FetchMessagesResponse { messages }))
}

/// POST /v1/messages/:msg_id/read
/// Marks a message as read for the reader client.
pub async fn handler_mark_read(
    State(state): State<AppState>,
    Path((msg_id,)): Path<(Uuid, )>, // Path<Uuid> for msg_id
    Json(req): Json<MarkReadRequest>,
) -> Result<Json<serde_json::Value>, impl IntoResponse> {
    state
        .instant_msg
        .mark_read_instant_message(req.reader_client_id, msg_id)
        .await
        .map_err(|e| {
            error!(msg_id = %msg_id, error = %e, "Mark read failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    info!(msg_id = %msg_id, reader = %req.reader_client_id, "API: Message marked read");
    Ok(Json(serde_json::json!({"status": "marked_as_read"})))
}

/// GET /v1/messages/:msg_id/receipts
/// Fetches read receipts for a message.
pub async fn handler_fetch_receipts(
    State(state): State<AppState>,
    Path(msg_id): Path<Uuid>,
) -> Result<Json<FetchReceiptsResponse>, impl IntoResponse> {
    let receipts = state
        .instant_msg
        .fetch_read_receipts(msg_id)
        .await
        .map_err(|e| {
            error!(msg_id = %msg_id, error = %e, "Fetch receipts failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    info!(msg_id = %msg_id, count = receipts.len(), "API: Receipts fetched");
    Ok(Json(FetchReceiptsResponse { receipts }))
}