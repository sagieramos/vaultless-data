use crate::AppState;
use crate::middleware::client::SessionDataClientExt;
use async_trait::async_trait;
use axum::{
    extract::{
        State,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use dashmap::DashMap;
use futures::{SinkExt, stream::StreamExt};
use redis::{AsyncCommands, Msg, RedisError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;
use vaultless_core::models::message::dto::InstantMessage;

// =============================================================================
// WebSocket Message Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsOutboundMessage {
    /// New message received
    NewMessage {
        message_id: Uuid,
        sender_id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        preview: Option<String>,
        timestamp: String,
    },
    /// Message read receipt
    ReadReceipt {
        message_id: Uuid,
        reader_id: Uuid,
        timestamp: String,
    },
    /// Typing indicator
    TypingIndicator { sender_id: Uuid, is_typing: bool },
    /// Connection acknowledged
    Connected { client_id: Uuid, session_id: String },
    /// Heartbeat/ping
    Ping { timestamp: String },
    /// Error notification
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsInboundMessage {
    /// Subscribe to messages
    Subscribe { client_id: Uuid },
    /// Send typing indicator
    Typing { recipient_id: Uuid, is_typing: bool },
    /// Heartbeat response
    Pong,
    /// Mark message as read (via WebSocket)
    MarkRead { message_id: Uuid },
}

// =============================================================================
// WebSocket Manager
// =============================================================================

/// Manages all active WebSocket connections
pub struct WsManager {
    /// client_id -> broadcast sender
    connections: Arc<DashMap<Uuid, broadcast::Sender<WsOutboundMessage>>>,
    /// Redis pool for pub/sub
    redis_url: String,
    /// InstantMessage service reference
    instant_message: Arc<InstantMessage>,
}

impl WsManager {
    pub fn new(redis_url: String, instant_message: Arc<InstantMessage>) -> Arc<Self> {
        let manager = Arc::new(Self {
            connections: Arc::new(DashMap::new()),
            redis_url,
            instant_message,
        });

        // Spawn Redis pub/sub listener
        manager.spawn_pubsub_listener();

        manager
    }

    /// Register a new WebSocket connection
    pub fn register(&self, client_id: Uuid) -> broadcast::Receiver<WsOutboundMessage> {
        let (tx, rx) = broadcast::channel(100);
        self.connections.insert(client_id, tx);

        tracing::info!(
            client_id = %client_id,
            total_connections = self.connections.len(),
            "WebSocket connection registered"
        );

        rx
    }

    /// Unregister a WebSocket connection
    pub fn unregister(&self, client_id: &Uuid) {
        self.connections.remove(client_id);

        tracing::info!(
            client_id = %client_id,
            total_connections = self.connections.len(),
            "WebSocket connection unregistered"
        );
    }

    /// Send message to specific client
    pub fn send_to_client(&self, client_id: &Uuid, message: WsOutboundMessage) -> bool {
        if let Some(tx) = self.connections.get(client_id) {
            match tx.send(message) {
                Ok(_) => true,
                Err(e) => {
                    tracing::warn!(
                        client_id = %client_id,
                        error = %e,
                        "Failed to send WebSocket message"
                    );
                    false
                }
            }
        } else {
            false
        }
    }

    /// Broadcast to multiple clients
    pub fn broadcast_to_clients(&self, client_ids: &[Uuid], message: WsOutboundMessage) {
        for client_id in client_ids {
            self.send_to_client(client_id, message.clone());
        }
    }

    /// Get connection count
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Check if client is connected
    pub fn is_connected(&self, client_id: &Uuid) -> bool {
        self.connections.contains_key(client_id)
    }

    /// Spawn Redis Pub/Sub listener for cross-server notifications
    fn spawn_pubsub_listener(self: &Arc<Self>) {
        let manager = Arc::clone(self);

        tokio::spawn(async move {
            loop {
                match manager.run_pubsub_listener().await {
                    Ok(_) => {
                        tracing::warn!("Pub/Sub listener exited normally, restarting...");
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "Pub/Sub listener error, restarting in 5s..."
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }

    async fn run_pubsub_listener(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = redis::Client::open(self.redis_url.as_ref())?;

        loop {
            match self.try_pubsub_loop(&client).await {
                Ok(_) => {
                    tracing::warn!("Pub/Sub loop exited normally");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Pub/Sub error, reconnecting in 5s");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn try_pubsub_loop(
        &self,
        client: &redis::Client,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut pubsub = client.get_async_pubsub().await?;

        pubsub.psubscribe("message:*").await?;
        pubsub.psubscribe("read:*").await?;

        tracing::info!("WebSocket pub/sub listener started");

        let mut stream = pubsub.into_on_message();

        while let Some(msg) = stream.next().await {
            let channel: String = msg.get_channel_name().to_string();
            let payload: String = msg.get_payload().unwrap_or_default();

            self.handle_pubsub_message(&channel, &payload).await;
        }

        Ok(())
    }

    async fn handle_pubsub_message(&self, channel: &str, payload: &str) {
        if let Some(prefix) = channel.strip_prefix("message:") {
            // New message notification: message:{recipient_id}
            if let Ok(recipient_id) = Uuid::parse_str(prefix) {
                if let Ok(sender_id) = Uuid::parse_str(payload) {
                    let message = WsOutboundMessage::NewMessage {
                        message_id: Uuid::new_v4(), // Will be replaced by actual ID
                        sender_id,
                        preview: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    };

                    self.send_to_client(&recipient_id, message);
                }
            }
        } else if let Some(prefix) = channel.strip_prefix("read:") {
            // Read receipt: read:{message_id}
            if let Ok(message_id) = Uuid::parse_str(prefix) {
                if let Ok(reader_id) = Uuid::parse_str(payload) {
                    // Notify the sender that their message was read
                    // (Would need to look up sender from message_id)
                    let message = WsOutboundMessage::ReadReceipt {
                        message_id,
                        reader_id,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    };

                    // Would need to broadcast to sender
                    // self.send_to_client(&sender_id, message);
                }
            }
        }
    }
}

// =============================================================================
// Integration with InstantMessage Service
// =============================================================================

#[async_trait]
pub trait InstantMessageExt {
    async fn notify_message_sent(
        &self,
        ws_manager: &WsManager,
        message_id: Uuid,
        sender_id: Uuid,
        recipient_id: Uuid,
    );

    async fn notify_message_read(
        &self,
        ws_manager: &WsManager,
        message_id: Uuid,
        reader_id: Uuid,
        sender_id: Uuid,
    );
}

#[async_trait]
impl InstantMessageExt for InstantMessage {
    async fn notify_message_sent(
        &self,
        ws_manager: &WsManager,
        message_id: Uuid,
        sender_id: Uuid,
        recipient_id: Uuid,
    ) {
        let notification = WsOutboundMessage::NewMessage {
            message_id,
            sender_id,
            preview: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        ws_manager.send_to_client(&recipient_id, notification);
    }

    async fn notify_message_read(
        &self,
        ws_manager: &WsManager,
        message_id: Uuid,
        reader_id: Uuid,
        sender_id: Uuid,
    ) {
        let notification = WsOutboundMessage::ReadReceipt {
            message_id,
            reader_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        ws_manager.send_to_client(&sender_id, notification);
    }
}
