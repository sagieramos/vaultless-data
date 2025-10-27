//! High-performance Instant Message Model
//!
//! - Caches incoming messages in Redis for instant reads.
//! - Batches flushes to Postgres every minute.
//! - Removes P2P messages after recipient fetches them.
//! - Verifies sender and recipient credentials before insert.
//! - Scales horizontally — safe for WebSocket, MQTT, gRPC, GraphQL gateways.

use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::interval};
use tracing::{error, info};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};
use crate::models::client::Client;
use crate::usage::{MetricsConfig, increment_message_received_pool, increment_message_sent_pool}; // For verify_client_credentials

// =============================================================================
// Configuration
// =============================================================================

const CACHE_TTL_SECS: u64 = 600;
const FLUSH_INTERVAL_SECS: u64 = 60;
const MAX_BATCH_SIZE: usize = 2000;
const CHANNEL_BUFFER: usize = 20_000; // Supports high concurrency

// =============================================================================
// Message Struct
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub ciphertext: String,
    pub nonce: String,
    pub sender_client_id: Uuid,
    pub recipient_client_id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub is_group_message: bool,

    pub content_size_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
}

// =============================================================================
// InstantMessage Core
// =============================================================================

#[derive(Clone)]
pub struct InstantMessage {
    redis_pool: Arc<RedisPool>,
    db_pool: Arc<PgPool>,
    config: MetricsConfig,
    sender: mpsc::Sender<Message>,
}

impl InstantMessage {
    pub fn new(redis_pool: RedisPool, db_pool: PgPool, config: MetricsConfig) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        let this = Self {
            redis_pool: Arc::new(redis_pool),
            db_pool: Arc::new(db_pool),
            config,
            sender: tx,
        };
        this.spawn_flusher(rx);
        this
    }

    // -------------------------------------------------------------------------
    // Send (create) instant message
    // -------------------------------------------------------------------------
    pub async fn send_instant_message(
        &self,
        sender_client_id: Uuid,
        recipient_client_id: Uuid,
        ciphertext: String,
        api_key_id: Option<Uuid>,
        size_bytes: i64,
        sender_identifier_hash: &str,
        sender_public_key: &str,
        recipient_identifier_hash: &str,
        recipient_public_key: &str,
    ) -> Result<Uuid> {
        // Corrected calls in instant_message.rs:
        let sender_ok = Client::verify_client_credentials(
            &*self.db_pool,
            &self.redis_pool,
            sender_identifier_hash,
            sender_public_key,
            None,
        )
        .await?;

        let recipient_ok = Client::verify_client_credentials(
            &*self.db_pool,
            &self.redis_pool,
            recipient_identifier_hash,
            recipient_public_key,
            None,
        )
        .await?;
        if !sender_ok || !recipient_ok {
            return Err(VaultlessError::Unauthorized(
                "Invalid client credentials".into(),
            ));
        }

        // 2. Build message
        let msg = Message {
            id: Uuid::new_v4(),
            ciphertext,
            nonce: Uuid::new_v4().to_string(),
            sender_client_id,
            recipient_client_id,
            api_key_id,
            is_group_message: false,
            content_size_bytes: size_bytes,
            created_at: Utc::now(),
            is_delivered: false,
            delivered_at: None,
        };

        // 3. Queue for flush
        if let Err(_) = self.sender.try_send(msg.clone()) {
            return Err(VaultlessError::Internal("Queue full".into()));
        }

        // 4. Cache in Redis
        let mut conn = self.redis_pool.get().await?;
        let redis_key = format!("msg:{}", msg.id);
        let recipient_queue = format!("inbox:{}", msg.recipient_client_id);

        let serialized = serde_json::to_string(&msg)?;
        let _: () = redis::pipe()
            .atomic()
            .set_ex(&redis_key, &serialized, CACHE_TTL_SECS)
            .rpush(&recipient_queue, msg.id.to_string())
            .expire(&recipient_queue, CACHE_TTL_SECS as i64)
            .query_async(&mut conn)
            .await?;

        // 5. Metrics
        if let Some(api_key) = msg.api_key_id {
            increment_message_sent_pool(&self.redis_pool, api_key, size_bytes, &self.config)
                .await?;
        }

        Ok(msg.id)
    }

    // -------------------------------------------------------------------------
    // Fetch all pending messages for recipient
    // -------------------------------------------------------------------------
    pub async fn fetch_messages_for_recipient(
        &self,
        recipient_client_id: Uuid,
    ) -> Result<Vec<Message>> {
        let mut conn = self.redis_pool.get().await?;
        let queue_key = format!("inbox:{}", recipient_client_id);
        let msg_ids: Vec<String> = conn.lrange(&queue_key, 0, -1).await?;

        let mut messages = Vec::with_capacity(msg_ids.len());
        for msg_id in msg_ids.iter() {
            let redis_key = format!("msg:{}", msg_id);

            let data_result: std::result::Result<Option<String>, redis::RedisError> =
                conn.get(&redis_key).await;

            // Propagate the Redis error if it occurred
            if data_result.is_err() {
                return Err(data_result.unwrap_err().into());
            }

            // Now, safely check if data was found (i.e., if it was Some(data))
            if let Some(data) = data_result.unwrap() {
                let msg: Message = serde_json::from_str(&data)?;

                if let Some(api_key_id) = msg.api_key_id {
                    increment_message_received_pool(
                        &self.redis_pool,
                        api_key_id,
                        msg.content_size_bytes,
                        &self.config,
                    )
                    .await?;
                }
                messages.push(msg.clone());

                // Delete from Redis and SQL (since delivered)
                let _: () = conn.del(&redis_key).await?;
                let _ = sqlx::query("DELETE FROM messages WHERE id = $1")
                    .bind(msg.id)
                    .execute(&*self.db_pool)
                    .await;
            }
        }

        // Clear recipient inbox
        let _: () = conn.del(&queue_key).await?;

        Ok(messages)
    }

    // -------------------------------------------------------------------------
    // Background flusher
    // -------------------------------------------------------------------------
    fn spawn_flusher(&self, mut rx: mpsc::Receiver<Message>) {
        let db_pool = Arc::clone(&self.db_pool);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(FLUSH_INTERVAL_SECS));
            let mut buffer: Vec<Message> = Vec::with_capacity(MAX_BATCH_SIZE);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !buffer.is_empty() {
                            if let Err(e) = flush_batch(&db_pool, &mut buffer).await {
                                error!("Flush error: {:?}", e);
                            }
                        }
                    }
                    Some(msg) = rx.recv() => {
                        buffer.push(msg);
                        if buffer.len() >= MAX_BATCH_SIZE {
                            if let Err(e) = flush_batch(&db_pool, &mut buffer).await {
                                error!("Immediate flush failed: {:?}", e);
                            }
                        }
                    }
                    else => break,
                }
            }

            if !buffer.is_empty() {
                let _ = flush_batch(&db_pool, &mut buffer).await;
            }

            info!("InstantMessage flusher stopped");
        });
    }
}

// =============================================================================
// Batch Insert
// =============================================================================

async fn flush_batch(db_pool: &PgPool, buffer: &mut Vec<Message>) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }

    let mut tx = db_pool.begin().await?;
    for msg in buffer.drain(..) {
        sqlx::query(
            r#"
            INSERT INTO messages (
                id, ciphertext, nonce, sender_client_id, recipient_client_id,
                api_key_id, is_group_message, content_size_bytes, created_at,
                is_delivered, delivered_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            ON CONFLICT (id) DO NOTHING
        "#,
        )
        .bind(msg.id)
        .bind(msg.ciphertext)
        .bind(msg.nonce)
        .bind(msg.sender_client_id)
        .bind(msg.recipient_client_id)
        .bind(msg.api_key_id)
        .bind(msg.is_group_message)
        .bind(msg.content_size_bytes)
        .bind(msg.created_at)
        .bind(msg.is_delivered)
        .bind(msg.delivered_at)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    info!("Flushed batch of messages to DB");
    Ok(())
}
