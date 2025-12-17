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
use vaultless_core::models::message::dto::InstantMessage;

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
        api_key_id = %app.sk_id,
        app_id = %app.app_id,
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
    let api_key_id = app.sk_id;

    // Register connection
    let mut rx = state.ws_manager.register(client_id);

    // Split socket into sender and receiver
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Send connection acknowledgment
    let ack = WsOutboundMessage::Connected {
        client_id,
        session_id: Uuid::new_v4().to_string(),
    };

    if let Ok(json) = serde_json::to_string(&ack) {
        let _ = sender.lock().await.send(WsMessage::Text(json.into())).await;
    }

    // Spawn heartbeat task
    let heartbeat_handle = {
        let sender_clone = Arc::clone(&sender);
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
        let sender_clone = Arc::clone(&sender);
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

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                // ✅ Validate quota BEFORE processing each message
                match app.validate_hot(redis_pool.clone()).await {
                    Ok(_) => {
                        handle_inbound_message(
                            &text.to_string(),
                            client_id,
                            api_key_id,
                            &instant_message,
                            &ws_manager,
                        )
                        .await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            client_id = %client_id,
                            api_key_id = %api_key_id,
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
                            let _ = sender
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
                    api_key_id = %api_key_id,
                    "WebSocket close received"
                );
                break;
            }
            Err(e) => {
                tracing::warn!(
                    client_id = %client_id,
                    api_key_id = %api_key_id,
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
        api_key_id = %api_key_id,
        "WebSocket connection closed"
    );
}

/// Handle inbound WebSocket messages
async fn handle_inbound_message(
    text: &str,
    client_id: Uuid,
    api_key_id: Uuid,
    instant_message: &Arc<InstantMessage>,
    ws_manager: &Arc<WsManager>,
) {
    let msg: WsInboundMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                client_id = %client_id,
                api_key_id = %api_key_id,
                error = %e,
                "Failed to parse WebSocket message"
            );
            return;
        }
    };

    match msg {
        WsInboundMessage::Subscribe { client_id: sub_id } => {
            if sub_id != client_id {
                tracing::warn!(
                    client_id = %client_id,
                    api_key_id = %api_key_id,
                    attempted_sub = %sub_id,
                    "Client attempted to subscribe to another client's messages"
                );
                return;
            }

            tracing::debug!(
                client_id = %client_id,
                api_key_id = %api_key_id,
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
                api_key_id = %api_key_id,
                recipient_id = %recipient_id,
                is_typing = is_typing,
                "Typing indicator sent"
            );
        }

        WsInboundMessage::MarkRead { message_id } => {
            // ✅ This will internally use the same quota tracking as HTTP
            if let Err(e) = instant_message
                .mark_read_instant_message(client_id, message_id)
                .await
            {
                tracing::error!(
                    client_id = %client_id,
                    api_key_id = %api_key_id,
                    message_id = %message_id,
                    error = %e,
                    "Failed to mark message as read via WebSocket"
                );
            } else {
                tracing::debug!(
                    client_id = %client_id,
                    api_key_id = %api_key_id,
                    message_id = %message_id,
                    "Message marked as read via WebSocket"
                );
            }
        }

        WsInboundMessage::Pong => {
            tracing::trace!(
                client_id = %client_id,
                api_key_id = %api_key_id,
                "Heartbeat pong received"
            );
        }
    }
}