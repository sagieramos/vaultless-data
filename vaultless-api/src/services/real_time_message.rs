use async_trait::async_trait;
use chrono;
use dashmap::DashMap;
use futures::stream::StreamExt;
use redis;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{self, sync::broadcast};
use uuid::Uuid;
use vaultless_core::models::message::dto::InstantMessage;

// =============================================================================
// WebSocket Message Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsOutboundMessage {
    /// New message notification (lightweight - client should fetch full message)
    NewMessage {
        message_id: Uuid,
        sender_id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        preview: Option<String>,
        timestamp: String,
    },
    /// Full message delivered via WebSocket (includes encrypted content)
    MessageDelivered {
        message_id: Uuid,
        sender_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        signature: Option<String>,
        content_size_bytes: i64,
        created_at: String,
    },
    /// Message send confirmation (response to SendMessage)
    MessageSent {
        message_id: Uuid,
        recipient_id: Uuid,
        recipient_online: bool,
        created_at: String,
    },
    /// Inbox messages response (response to FetchInbox)
    InboxMessages {
        messages: Vec<WsMessageDto>,
        count: usize,
    },
    /// Inbox status notification (sent on connect)
    InboxStatus {
        /// Number of unread/pending messages
        unread_count: usize,
        /// Timestamp of oldest unread message (if any)
        #[serde(skip_serializing_if = "Option::is_none")]
        oldest_unread_at: Option<String>,
        /// Timestamp of newest unread message (if any)
        #[serde(skip_serializing_if = "Option::is_none")]
        newest_unread_at: Option<String>,
        /// Total size of pending messages in bytes
        total_size_bytes: i64,
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
    /// Handshake initiated - peer metadata returned
    HandshakeInitiated {
        peer_signing_key: String,
        peer_identifier: Option<String>,
    },
    /// Handshake response stored
    HandshakeRespondSuccess {
        session_id: String,
        expires_at: String,
    },
    /// Handshake completed - session stored
    HandshakeCompleted {
        session_id: String,
        expires_at: String,
    },
    /// Error notification
    Error { code: String, message: String },
}

/// Lightweight message DTO for WebSocket transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessageDto {
    pub id: Uuid,
    pub sender_id: Uuid,
    pub ciphertext: String,
    pub nonce: Uuid,
    pub signature: Option<String>,
    pub content_size_bytes: i64,
    pub created_at: String,
}

/// Handshake request data (matches API DTO)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequestData {
    pub handshake_id: String,
    pub signing_pubkey: String,
    pub ephemeral_exchange_pubkey: String,
    pub timestamp: String,
    pub signature: String,
}

/// Handshake response data (matches API DTO)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponseData {
    pub handshake_id: String,
    pub signing_pubkey: String,
    pub ephemeral_exchange_pubkey: String,
    pub timestamp: String,
    pub session_id: String,
    pub expires_at: String,
    pub signature: String,
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
    /// Send a message via WebSocket (P2P)
    SendMessage {
        /// Recipient identifier (username/email) - optional if pubkey provided
        recipient_identifier: Option<String>,
        /// Recipient public key - optional if identifier provided
        recipient_pubkey: Option<String>,
        /// Encrypted message content (base64 or hex)
        ciphertext: String,
        /// Nonce for encryption
        nonce: Uuid,
        /// Ed25519/P-256 signature of envelope
        signature: Option<String>,
        /// Whether to require proof verification (defaults to true)
        #[serde(default = "default_require_verification")]
        require_proof_verification: bool,
    },
    /// Fetch inbox messages via WebSocket
    FetchInbox {
        /// Optional limit
        limit: Option<usize>,
    },
    /// Initiate handshake with peer (lookup peer metadata)
    InitiateHandshake {
        /// Peer identifier or signing key
        peer_identifier: Option<String>,
        peer_signing_key: Option<String>,
    },
    /// Store session after responding to handshake
    RespondHandshake {
        /// Handshake request data (for verification)
        handshake_request: HandshakeRequestData,
        /// Session ID
        session_id: String,
        /// Responder's ephemeral public key
        ephemeral_public_key: String,
        /// Session expiry
        expires_at: String,
    },
    /// Store session after completing handshake
    CompleteHandshake {
        /// Handshake response data (for verification)
        handshake_response: HandshakeResponseData,
        /// Expected handshake ID
        expected_handshake_id: String,
        /// Initiator's ephemeral public key
        ephemeral_public_key: String,
    },
}

fn default_require_verification() -> bool {
    true
}

/// Pub/Sub message payload for cross-server message delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubSubMessagePayload {
    pub message_id: Uuid,
    pub sender_id: Uuid,
    pub ciphertext: String,
    pub nonce: Uuid,
    pub signature: Option<String>,
    pub content_size_bytes: i64,
    pub created_at: String,
}

/// Pub/Sub read receipt payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubSubReadReceiptPayload {
    pub message_id: Uuid,
    pub reader_id: Uuid,
    pub sender_id: Uuid,
    pub timestamp: String,
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

    /// Publish message delivery notification via Redis pub/sub (for cross-server)
    pub async fn publish_message_delivery(
        &self,
        recipient_id: Uuid,
        payload: PubSubMessagePayload,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = redis::Client::open(self.redis_url.as_ref())?;
        let mut conn = client.get_multiplexed_async_connection().await?;

        let channel = format!("message:{}", recipient_id);
        let payload_json = serde_json::to_string(&payload)?;

        let _: () = redis::cmd("PUBLISH")
            .arg(&channel)
            .arg(&payload_json)
            .query_async(&mut conn)
            .await?;

        tracing::debug!(
            recipient_id = %recipient_id,
            message_id = %payload.message_id,
            "Published message delivery to pub/sub"
        );

        Ok(())
    }

    /// Publish read receipt via Redis pub/sub (for cross-server)
    pub async fn publish_read_receipt(
        &self,
        payload: PubSubReadReceiptPayload,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let client = redis::Client::open(self.redis_url.as_ref())?;
        let mut conn = client.get_multiplexed_async_connection().await?;

        let channel = format!("read:{}", payload.message_id);
        let payload_json = serde_json::to_string(&payload)?;

        let _: () = redis::cmd("PUBLISH")
            .arg(&channel)
            .arg(&payload_json)
            .query_async(&mut conn)
            .await?;

        tracing::debug!(
            message_id = %payload.message_id,
            reader_id = %payload.reader_id,
            "Published read receipt to pub/sub"
        );

        Ok(())
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
                // Try to parse as full payload first (new format)
                if let Ok(msg_payload) = serde_json::from_str::<PubSubMessagePayload>(payload) {
                    // Full message delivery with content
                    let message = WsOutboundMessage::MessageDelivered {
                        message_id: msg_payload.message_id,
                        sender_id: msg_payload.sender_id,
                        ciphertext: msg_payload.ciphertext,
                        nonce: msg_payload.nonce,
                        signature: msg_payload.signature,
                        content_size_bytes: msg_payload.content_size_bytes,
                        created_at: msg_payload.created_at,
                    };
                    self.send_to_client(&recipient_id, message);
                } else if let Ok(sender_id) = Uuid::parse_str(payload) {
                    // Legacy format: just sender_id (lightweight notification)
                    let message = WsOutboundMessage::NewMessage {
                        message_id: Uuid::new_v4(),
                        sender_id,
                        preview: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    };
                    self.send_to_client(&recipient_id, message);
                } else {
                    tracing::warn!(
                        channel = %channel,
                        "Failed to parse message pub/sub payload"
                    );
                }
            }
        } else if let Some(_prefix) = channel.strip_prefix("read:") {
            // Read receipt: read:{message_id}
            // Try to parse as full payload first (new format with sender_id)
            if let Ok(receipt_payload) = serde_json::from_str::<PubSubReadReceiptPayload>(payload) {
                let message = WsOutboundMessage::ReadReceipt {
                    message_id: receipt_payload.message_id,
                    reader_id: receipt_payload.reader_id,
                    timestamp: receipt_payload.timestamp,
                };
                // Send to the original sender so they know their message was read
                self.send_to_client(&receipt_payload.sender_id, message);
            } else {
                tracing::warn!(
                    channel = %channel,
                    "Failed to parse read receipt pub/sub payload (missing sender_id)"
                );
            }
        }
    }
}

// =============================================================================
// Integration with InstantMessage Service
// =============================================================================

#[async_trait]
pub trait InstantMessageExt {
    /// Notify recipient of new message (lightweight notification)
    async fn notify_message_sent(
        &self,
        ws_manager: &WsManager,
        message_id: Uuid,
        sender_id: Uuid,
        recipient_id: Uuid,
    );

    /// Deliver full message content to recipient via WebSocket
    async fn deliver_message_content(
        &self,
        ws_manager: &WsManager,
        recipient_id: Uuid,
        message_id: Uuid,
        sender_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        signature: Option<String>,
        content_size_bytes: i64,
    );

    /// Notify sender of read receipt
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

        // Send locally first
        let sent_locally = ws_manager.send_to_client(&recipient_id, notification.clone());

        // Also publish to Redis for cross-server delivery
        if !sent_locally {
            let payload = PubSubMessagePayload {
                message_id,
                sender_id,
                ciphertext: String::new(), // Empty for lightweight notification
                nonce: Uuid::nil(),
                signature: None,
                content_size_bytes: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            if let Err(e) = ws_manager
                .publish_message_delivery(recipient_id, payload)
                .await
            {
                tracing::error!(
                    recipient_id = %recipient_id,
                    message_id = %message_id,
                    error = %e,
                    "Failed to publish message notification to pub/sub"
                );
            }
        }
    }

    async fn deliver_message_content(
        &self,
        ws_manager: &WsManager,
        recipient_id: Uuid,
        message_id: Uuid,
        sender_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        signature: Option<String>,
        content_size_bytes: i64,
    ) {
        let created_at = chrono::Utc::now().to_rfc3339();

        let notification = WsOutboundMessage::MessageDelivered {
            message_id,
            sender_id,
            ciphertext: ciphertext.clone(),
            nonce,
            signature: signature.clone(),
            content_size_bytes,
            created_at: created_at.clone(),
        };

        // Send locally first
        let sent_locally = ws_manager.send_to_client(&recipient_id, notification);

        // Also publish to Redis for cross-server delivery
        if !sent_locally {
            let payload = PubSubMessagePayload {
                message_id,
                sender_id,
                ciphertext,
                nonce,
                signature,
                content_size_bytes,
                created_at,
            };

            if let Err(e) = ws_manager
                .publish_message_delivery(recipient_id, payload)
                .await
            {
                tracing::error!(
                    recipient_id = %recipient_id,
                    message_id = %message_id,
                    error = %e,
                    "Failed to publish message delivery to pub/sub"
                );
            }
        }
    }

    async fn notify_message_read(
        &self,
        ws_manager: &WsManager,
        message_id: Uuid,
        reader_id: Uuid,
        sender_id: Uuid,
    ) {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let notification = WsOutboundMessage::ReadReceipt {
            message_id,
            reader_id,
            timestamp: timestamp.clone(),
        };

        // Send locally first
        let sent_locally = ws_manager.send_to_client(&sender_id, notification);

        // Also publish to Redis for cross-server delivery
        if !sent_locally {
            let payload = PubSubReadReceiptPayload {
                message_id,
                reader_id,
                sender_id,
                timestamp,
            };

            if let Err(e) = ws_manager.publish_read_receipt(payload).await {
                tracing::error!(
                    message_id = %message_id,
                    reader_id = %reader_id,
                    error = %e,
                    "Failed to publish read receipt to pub/sub"
                );
            }
        }
    }
}
