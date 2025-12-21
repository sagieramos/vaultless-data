use crate::AppState;
use crate::middleware::{
    application::ApplicationKeyViewExt,
    client::SessionDataClientExt,
};
use crate::services::real_time_message::*;
use axum::{
    extract::{
        State,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use futures::{SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use chrono;
use serde_json;
use tokio;
use vaultless_core::models::message::dto::InstantMessage;
use vaultless_core::Client;
use deadpool_redis::Pool as RedisPool;

/// WebSocket upgrade handler
/// GET /api/messages/ws
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    SessionDataClientExt(client_info): SessionDataClientExt,
    ApplicationKeyViewExt(app): ApplicationKeyViewExt, 
) -> Response {
    tracing::info!(
        client_id = %client_info.client_id,
        application_id = %app.app_id,
        "WebSocket upgrade request (quota already validated)"
    );

    // ✅ Quota already validated by app_auth middleware
    // Just pass validated app to socket handler
    ws.on_upgrade(move |socket| handle_socket(socket, state, client_info, app))
}

/// Handle individual WebSocket connection
async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    client_info: vaultless_core::SessionData,
    app: Arc<vaultless_core::ApplicationKeyView>,
) {
    let client_id = client_info.client_id;
    // Use application_id (stable across API key rotations) for metrics tracking
    let application_id = app.app_id;
    let sender_pubkey = client_info.pubkey.clone();

    // Register connection
    let mut rx = state.ws_manager.register(client_id);

    // Split socket into sender and receiver
    let (ws_sender, mut receiver) = socket.split();
    let ws_sender = Arc::new(Mutex::new(ws_sender));

    // Send connection acknowledgment
    let ack = WsOutboundMessage::Connected {
        client_id,
        session_id: Uuid::new_v4().to_string(),
    };

    if let Ok(json) = serde_json::to_string(&ack) {
        let _ = ws_sender.lock().await.send(WsMessage::Text(json.into())).await;
    }

    // Send inbox status notification (hybrid approach - let client decide what to do)
    {
        let instant_message = Arc::clone(&state.instant_message);
        let ws_sender_clone = Arc::clone(&ws_sender);

        // Spawn as a separate task to not block connection setup
        tokio::spawn(async move {
            match instant_message.get_inbox_status(client_id).await {
                Ok(status) => {
                    let inbox_status = WsOutboundMessage::InboxStatus {
                        unread_count: status.unread_count,
                        oldest_unread_at: status.oldest_unread_at.map(|t| t.to_rfc3339()),
                        newest_unread_at: status.newest_unread_at.map(|t| t.to_rfc3339()),
                        total_size_bytes: status.total_size_bytes,
                    };

                    if let Ok(json) = serde_json::to_string(&inbox_status) {
                        let _ = ws_sender_clone.lock().await.send(WsMessage::Text(json.into())).await;
                    }

                    tracing::debug!(
                        client_id = %client_id,
                        unread_count = status.unread_count,
                        total_size_bytes = status.total_size_bytes,
                        "Sent inbox status on connect"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        client_id = %client_id,
                        error = %e,
                        "Failed to get inbox status on connect"
                    );
                }
            }
        });
    }

    // Spawn heartbeat task
    let heartbeat_handle = {
        let sender_clone = Arc::clone(&ws_sender);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

            loop {
                interval.tick().await;

                let ping = WsOutboundMessage::Ping {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };

                if let Ok(json) = serde_json::to_string(&ping) {
                    let mut sender_guard = sender_clone.lock().await;
                    if sender_guard
                        .send(WsMessage::Text(json.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })
    };

    // Spawn outbound message handler (broadcast receiver)
    let outbound_handle = {
        let sender_clone = Arc::clone(&ws_sender);
        tokio::spawn(async move {
            while let Ok(msg) = rx.recv().await {
                if let Ok(json) = serde_json::to_string(&msg) {
                    let mut sender_guard = sender_clone.lock().await;
                    if sender_guard
                        .send(WsMessage::Text(json.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        })
    };

    // Handle inbound messages from client
    let instant_message = Arc::clone(&state.instant_message);
    let ws_manager = Arc::clone(&state.ws_manager);
    let redis_pool = Arc::clone(&state.redis_pool);
    let db_pool = Arc::clone(&state.db);
    let ws_sender_for_handler = Arc::clone(&ws_sender);

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                // ✅ Validate quota BEFORE processing each message
                match app.validate_hot(redis_pool.clone()).await {
                    Ok(_) => {
                        handle_inbound_message(
                            &text.to_string(),
                            client_id,
                            application_id,
                            sender_pubkey.clone(),
                            &instant_message,
                            &ws_manager,
                            &db_pool,
                            &redis_pool,
                            &ws_sender_for_handler,
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            client_id = %client_id,
                            application_id = %application_id,
                            error = %e,
                            "Quota validation failed for WebSocket message"
                        );

                        // Send error to client
                        let error_msg = WsOutboundMessage::Error {
                            code: match &e {
                                vaultless_core::error::VaultlessError::QuotaExceeded(_) => {
                                    "QUOTA_EXCEEDED".to_string()
                                }
                                vaultless_core::error::VaultlessError::RateLimitExceeded(_) => {
                                    "RATE_LIMIT_EXCEEDED".to_string()
                                }
                                _ => "VALIDATION_ERROR".to_string(),
                            },
                            message: e.to_string(),
                        };

                        if let Ok(json) = serde_json::to_string(&error_msg) {
                            let _ = ws_sender
                                .lock()
                                .await
                                .send(WsMessage::Text(json.into()))
                                .await;
                        }

                        // Don't close connection, just skip this message
                        continue;
                    }
                }
            }
            Ok(WsMessage::Close(_)) => {
                tracing::info!(
                    client_id = %client_id,
                    application_id = %application_id,
                    "WebSocket close received"
                );
                break;
            }
            Err(e) => {
                tracing::warn!(
                    client_id = %client_id,
                    application_id = %application_id,
                    error = %e,
                    "WebSocket error"
                );
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    heartbeat_handle.abort();
    outbound_handle.abort();
    state.ws_manager.unregister(&client_id);

    tracing::info!(
        client_id = %client_id,
        application_id = %application_id,
        "WebSocket connection closed"
    );
}

/// Helper to send a response back to the client
async fn send_ws_response<S>(
    ws_sender: &Arc<Mutex<S>>,
    message: WsOutboundMessage,
) where
    S: futures::Sink<WsMessage> + Unpin,
    S::Error: std::fmt::Debug,
{
    if let Ok(json) = serde_json::to_string(&message) {
        let _ = ws_sender.lock().await.send(WsMessage::Text(json.into())).await;
    }
}

/// Handle inbound WebSocket messages
async fn handle_inbound_message<S>(
    text: &str,
    client_id: Uuid,
    application_id: Uuid,
    sender_pubkey: Option<String>,
    instant_message: &Arc<InstantMessage>,
    ws_manager: &Arc<WsManager>,
    db_pool: &Arc<sqlx::PgPool>,
    redis_pool: &Arc<RedisPool>,
    ws_sender: &Arc<Mutex<S>>,
) where
    S: futures::Sink<WsMessage> + Unpin,
    S::Error: std::fmt::Debug,
{
    let msg: WsInboundMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                client_id = %client_id,
                application_id = %application_id,
                error = %e,
                "Failed to parse WebSocket message"
            );
            send_ws_response(ws_sender, WsOutboundMessage::Error {
                code: "PARSE_ERROR".to_string(),
                message: format!("Failed to parse message: {}", e),
            }).await;
            return;
        }
    };

    match msg {
        WsInboundMessage::Subscribe { client_id: sub_id } => {
            if sub_id != client_id {
                tracing::warn!(
                    client_id = %client_id,
                    application_id = %application_id,
                    attempted_sub = %sub_id,
                    "Client attempted to subscribe to another client's messages"
                );
                send_ws_response(ws_sender, WsOutboundMessage::Error {
                    code: "UNAUTHORIZED".to_string(),
                    message: "Cannot subscribe to another client's messages".to_string(),
                }).await;
                return;
            }

            tracing::debug!(
                client_id = %client_id,
                application_id = %application_id,
                "Client subscribed to messages"
            );
        }

        WsInboundMessage::Typing {
            recipient_id,
            is_typing,
        } => {
            let typing_msg = WsOutboundMessage::TypingIndicator {
                sender_id: client_id,
                is_typing,
            };

            ws_manager.send_to_client(&recipient_id, typing_msg);

            tracing::trace!(
                client_id = %client_id,
                application_id = %application_id,
                recipient_id = %recipient_id,
                is_typing = is_typing,
                "Typing indicator sent"
            );
        }

        WsInboundMessage::MarkRead { message_id } => {
            if let Err(e) = instant_message
                .mark_read_instant_message(client_id, message_id)
                .await
            {
                tracing::error!(
                    client_id = %client_id,
                    application_id = %application_id,
                    message_id = %message_id,
                    error = %e,
                    "Failed to mark message as read via WebSocket"
                );
                send_ws_response(ws_sender, WsOutboundMessage::Error {
                    code: "MARK_READ_FAILED".to_string(),
                    message: e.to_string(),
                }).await;
            } else {
                tracing::debug!(
                    client_id = %client_id,
                    application_id = %application_id,
                    message_id = %message_id,
                    "Message marked as read via WebSocket"
                );
            }
        }

        WsInboundMessage::Pong => {
            tracing::trace!(
                client_id = %client_id,
                application_id = %application_id,
                "Heartbeat pong received"
            );
        }

        // NEW: Send message via WebSocket
        WsInboundMessage::SendMessage {
            recipient_identifier,
            recipient_pubkey,
            ciphertext,
            nonce,
            signature,
            require_proof_verification,
        } => {
            // Validate signature is provided if verification required
            if require_proof_verification && signature.is_none() {
                send_ws_response(ws_sender, WsOutboundMessage::Error {
                    code: "SIGNATURE_REQUIRED".to_string(),
                    message: "Signature required when proof verification is enabled".to_string(),
                }).await;
                return;
            }

            // Validate sender pubkey exists
            let envelope_pubkey = match &sender_pubkey {
                Some(pk) => pk.clone(),
                None => {
                    tracing::error!(
                        client_id = %client_id,
                        "Sender public key not found in session"
                    );
                    send_ws_response(ws_sender, WsOutboundMessage::Error {
                        code: "SENDER_PUBKEY_MISSING".to_string(),
                        message: "Sender public key not found".to_string(),
                    }).await;
                    return;
                }
            };

            // Resolve recipient
            let recipient = match Client::resolve_client(
                db_pool.as_ref(),
                Some(redis_pool.clone()),
                recipient_pubkey.as_deref(),
                recipient_identifier.as_deref(),
                None,
            )
            .await
            {
                Ok(Some(r)) => r,
                Ok(None) => {
                    send_ws_response(ws_sender, WsOutboundMessage::Error {
                        code: "RECIPIENT_NOT_FOUND".to_string(),
                        message: "Recipient client not found".to_string(),
                    }).await;
                    return;
                }
                Err(e) => {
                    tracing::error!(
                        client_id = %client_id,
                        error = %e,
                        "Failed to resolve recipient"
                    );
                    send_ws_response(ws_sender, WsOutboundMessage::Error {
                        code: "RECIPIENT_LOOKUP_FAILED".to_string(),
                        message: e.to_string(),
                    }).await;
                    return;
                }
            };

            // Compute content size
            let content_size_bytes = ciphertext.len() as i64;

            tracing::info!(
                sender = %client_id,
                recipient = %recipient.id,
                size = content_size_bytes,
                requires_verification = require_proof_verification,
                "Sending instant message via WebSocket"
            );

            // Send message using the InstantMessage service
            match instant_message
                .send_instant_message(
                    client_id,
                    recipient.id,
                    ciphertext.clone(),
                    nonce,
                    content_size_bytes,
                    application_id,
                    signature.clone(),
                    envelope_pubkey,
                    require_proof_verification,
                )
                .await
            {
                Ok(message_id) => {
                    let recipient_online = ws_manager.is_connected(&recipient.id);
                    let created_at = chrono::Utc::now();

                    // Notify recipient if online (send full message content)
                    if recipient_online {
                        let delivery_msg = WsOutboundMessage::MessageDelivered {
                            message_id,
                            sender_id: client_id,
                            ciphertext,
                            nonce,
                            signature,
                            content_size_bytes,
                            created_at: created_at.to_rfc3339(),
                        };
                        ws_manager.send_to_client(&recipient.id, delivery_msg);
                    }

                    // Send confirmation to sender
                    send_ws_response(ws_sender, WsOutboundMessage::MessageSent {
                        message_id,
                        recipient_id: recipient.id,
                        recipient_online,
                        created_at: created_at.to_rfc3339(),
                    }).await;

                    tracing::info!(
                        sender = %client_id,
                        recipient = %recipient.id,
                        message_id = %message_id,
                        recipient_online = recipient_online,
                        "Message sent successfully via WebSocket"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        sender = %client_id,
                        recipient = %recipient.id,
                        error = %e,
                        "Failed to send message via WebSocket"
                    );
                    send_ws_response(ws_sender, WsOutboundMessage::Error {
                        code: "SEND_FAILED".to_string(),
                        message: e.to_string(),
                    }).await;
                }
            }
        }

        // NEW: Fetch inbox via WebSocket
        WsInboundMessage::FetchInbox { limit: _ } => {
            match instant_message.fetch_messages_for_recipient(client_id).await {
                Ok(messages) => {
                    let count = messages.len();
                    let ws_messages: Vec<WsMessageDto> = messages
                        .into_iter()
                        .map(|m| WsMessageDto {
                            id: m.id,
                            sender_id: m.sender_client_id,
                            ciphertext: m.ciphertext,
                            nonce: m.nonce,
                            signature: m.signature,
                            content_size_bytes: m.content_size_bytes,
                            created_at: m.created_at.to_rfc3339(),
                        })
                        .collect();

                    send_ws_response(ws_sender, WsOutboundMessage::InboxMessages {
                        messages: ws_messages,
                        count,
                    }).await;

                    tracing::info!(
                        client_id = %client_id,
                        count,
                        "Fetched inbox via WebSocket"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        client_id = %client_id,
                        error = %e,
                        "Failed to fetch inbox via WebSocket"
                    );
                    send_ws_response(ws_sender, WsOutboundMessage::Error {
                        code: "FETCH_INBOX_FAILED".to_string(),
                        message: e.to_string(),
                    }).await;
                }
            }
        }
    }
}