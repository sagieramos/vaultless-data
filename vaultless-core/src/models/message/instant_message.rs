//! High-performance Instant Message Model (P2P Only)
//!
//! - Caches incoming messages in Redis for instant reads.
//! - Batches flushes to Postgres every minute.
//! - Removes P2P messages after recipient fetches them.
//! - Verifies sender and recipient credentials before insert.
//! - Supports file attachments via encrypted metadata.
//! - Tracks read receipts for delivery confirmation.
//! - Scales horizontally — safe for WebSocket, MQTT, gRPC, GraphQL gateways.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Row, query_as, query_builder::QueryBuilder};
use std::{env, sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::interval};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::crypto::{PRIVATE_KEY_SIZE, verify_signature};
use crate::error::{Result, VaultlessError};
use crate::usage::{MetricsConfig, increment_message_received_pool};

// =============================================================================
// Configuration
// =============================================================================

const CACHE_TTL_SECS: u64 = 600;
const FLUSH_INTERVAL_SECS: u64 = 60;
const MAX_BATCH_SIZE: usize = 2000;
const CHANNEL_BUFFER: usize = 20_000; // Supports ~1s burst at 20k msg/s
const MAX_QUEUE_LEN: isize = 10_000; // Cap per-recipient queue to prevent bloat

const CLEANUP_INTERVAL_SECS: u64 = 10;
const MAX_DELETE_BATCH: usize = 1000;
const DELETE_CHANNEL_BUFFER: usize = 10_000; // Similar to CHANNEL_BUFFER

const PURGE_INTERVAL_HOURS: u64 = 1; // Check every hour for 24h purges
const RETENTION_AFTER_DELIVERY_HOURS: i64 = 24;

const CHUNK_SIZE_BYTES: usize = 1_000_000; // 1MB chunks for files
const MAX_FILE_SIZE_BYTES: i64 = 100_000_000; // 100MB max

// =============================================================================
// Envelope Helper (Module-level for reuse)
#[derive(Serialize)]
struct Envelope<'a> {
    id: &'a Uuid,
    sender_client_id: &'a Uuid,
    recipient_client_id: &'a Uuid,
    api_key_id: &'a Option<Uuid>,
    is_group_message: bool,
    content_size_bytes: i64,
    created_at: &'a DateTime<Utc>,
    is_delivered: bool,
    delivered_at: &'a Option<DateTime<Utc>>,
}

// =============================================================================
// Message Struct (Extended for Files)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub ciphertext: String,
    pub nonce: String,
    pub sender_client_id: Uuid,
    pub recipient_client_id: Uuid,
    pub api_key_id: Option<Uuid>,
    pub is_group_message: bool, // Always false for P2P

    pub content_size_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,

    pub signature: String,           // Base64 Ed25519 signature of envelope
    pub envelope_public_key: String, // Base64 Ed25519 public key (server-derived)

    // New: File attachment
    pub file_id: Option<Uuid>, // Links to group_files (group_id=NULL for P2P)
}

// =============================================================================
// File Struct (From Schema)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct P2PFile {
    pub id: Uuid,
    pub message_id: Option<Uuid>,
    pub uploader_client_id: Uuid,
    pub encrypted_filename: String,
    pub encrypted_mime_type: String,
    pub file_size_bytes: i64,
    pub encrypted_file_key: String,
    pub nonce: String,
    pub storage_path: String,
    pub chunk_count: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub download_count: i32,
    pub max_downloads: Option<i32>,
}

// =============================================================================
// Read Receipt Struct (Adapted for P2P)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReadReceipt {
    pub id: Uuid,
    pub message_id: Uuid,
    pub client_id: Uuid, // Reader (recipient)
    pub read_at: DateTime<Utc>,
}

// =============================================================================
// Delete Task
// =============================================================================

#[derive(Debug, Clone)]
struct DeleteTask {
    msg_id: Uuid,
    is_group_message: bool,
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
    delete_sender: mpsc::Sender<DeleteTask>,
    server_private_key: [u8; PRIVATE_KEY_SIZE], // Server signing key
}

impl InstantMessage {
    pub fn new(redis_pool: RedisPool, db_pool: PgPool, config: MetricsConfig) -> Result<Self> {
        // Load server private key from env (in prod: base64-encoded in VAULTLESS_SERVER_PRIVATE_KEY)
        let private_key_b64 = env::var("VAULTLESS_SERVER_PRIVATE_KEY").map_err(|_| {
            VaultlessError::Internal("Missing env VAULTLESS_SERVER_PRIVATE_KEY".into())
        })?;
        let private_key_bytes = BASE64.decode(&private_key_b64).map_err(|e| {
            VaultlessError::Validation(format!("Invalid base64 private key: {}", e))
        })?;
        if private_key_bytes.len() != PRIVATE_KEY_SIZE {
            return Err(VaultlessError::Validation(format!(
                "Private key must be {} bytes, got {}",
                PRIVATE_KEY_SIZE,
                private_key_bytes.len()
            )));
        }
        let mut server_private_key = [0u8; PRIVATE_KEY_SIZE];
        server_private_key.copy_from_slice(&private_key_bytes);

        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        let (delete_tx, delete_rx) = mpsc::channel(DELETE_CHANNEL_BUFFER);
        let this = Self {
            redis_pool: Arc::new(redis_pool),
            db_pool: Arc::new(db_pool),
            config,
            sender: tx,
            delete_sender: delete_tx,
            server_private_key,
        };
        this.spawn_flusher(rx);
        this.spawn_deleter(delete_rx);
        this.spawn_purger(); // New: For 24h delayed deletes
        Ok(this)
    }

    // ... (send_instant_message and send_file_instant_message unchanged; Envelope now module-level)

    // -------------------------------------------------------------------------
    // Mark message as read (P2P Receipt)
    // -------------------------------------------------------------------------
    pub async fn mark_read_instant_message(
        &self,
        reader_client_id: Uuid,
        msg_id: Uuid,
    ) -> Result<()> {
        // Verify reader is recipient
        let msg_row = sqlx::query_as::<_, (Uuid,)>(
            "SELECT recipient_client_id FROM messages WHERE id = $1"
        )
        .bind(msg_id)
        .fetch_optional(&*self.db_pool)
        .await?;

        let (recipient_id,) = msg_row.ok_or(VaultlessError::NotFound("Message not found".into()))?;

        if reader_client_id != recipient_id {
            return Err(VaultlessError::Unauthorized("Not recipient".into()));
        }

        // Insert receipt (group_id=NULL for P2P)
        let receipt_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO group_message_read_receipts (id, message_id, group_id, client_address, read_at)
            VALUES ($1, $2, NULL, $3, $4)
            ON CONFLICT (message_id, client_address) DO UPDATE SET read_at = $4
            "#,
        )
        .bind(receipt_id)
        .bind(msg_id)
        .bind(reader_client_id)
        .bind(Utc::now())
        .execute(&*self.db_pool)
        .await?;

        // Update message delivered_at if not set
        let _ = sqlx::query(
            "UPDATE messages SET delivered_at = $1 WHERE id = $2 AND delivered_at IS NULL",
        )
        .bind(Utc::now())
        .bind(msg_id)
        .execute(&*self.db_pool)
        .await?;

        // Pub/Sub for real-time (e.g., sender sees read)
        let mut conn = self.redis_pool.get().await?;
        let _: () = conn
            .publish(&format!("read:{}", msg_id), reader_client_id.to_string())
            .await?;

        info!("Marked message {} as read by {}", msg_id, reader_client_id);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Fetch read receipts for message (P2P)
    // -------------------------------------------------------------------------
    pub async fn fetch_read_receipts(&self, msg_id: Uuid) -> Result<Vec<ReadReceipt>> {
        let receipts = query_as::<_, ReadReceipt>(
            r#"
            SELECT id, message_id, client_address as client_id, read_at
            FROM group_message_read_receipts
            WHERE message_id = $1 AND group_id IS NULL
            "#,
        )
        .bind(msg_id)
        .fetch_all(&*self.db_pool)
        .await?;

        Ok(receipts)
    }

    // -------------------------------------------------------------------------
    // Fetch all pending messages for recipient (Updated for Files/Receipts)
    // -------------------------------------------------------------------------
    pub async fn fetch_messages_for_recipient(
        &self,
        recipient_client_id: Uuid,
    ) -> Result<Vec<Message>> {
        let mut conn = self.redis_pool.get().await?;
        let queue_key = format!("inbox:{}", recipient_client_id);
        let msg_ids: Vec<String> = conn.lrange(&queue_key, 0, -1).await?;

        let mut messages = Vec::with_capacity(msg_ids.len());
        let mut fallback_ids = Vec::new(); // IDs for SQL fallback

        for msg_id_str in msg_ids.iter() {
            let msg_id = match Uuid::parse_str(msg_id_str) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let redis_key = format!("msg:{}", msg_id);

            let data_opt: Option<String> = conn.get(&redis_key).await?;
            if let Some(data) = data_opt {
                // Redis fast path
                let msg: Message = serde_json::from_str(&data)?;

                // Verify envelope signature (using module-level Envelope)
                let envelope = Envelope {
                    id: &msg.id,
                    sender_client_id: &msg.sender_client_id,
                    recipient_client_id: &msg.recipient_client_id,
                    api_key_id: &msg.api_key_id,
                    is_group_message: msg.is_group_message,
                    content_size_bytes: msg.content_size_bytes,
                    created_at: &msg.created_at,
                    is_delivered: msg.is_delivered,
                    delivered_at: &msg.delivered_at,
                };
                let envelope_bytes = serde_json::to_vec(&envelope)?;

                if let Err(e) =
                    verify_signature(&envelope_bytes, &msg.signature, &msg.envelope_public_key)
                {
                    error!("Envelope verification failed for msg {}: {:?}", msg.id, e);
                    let _: () = conn.del(&redis_key).await?;
                    continue;
                }

                // Mark as delivered
                let mut delivered_msg = msg.clone();
                delivered_msg.is_delivered = true;
                delivered_msg.delivered_at = Some(Utc::now());

                // Metrics
                if let Some(api_key_id) = delivered_msg.api_key_id {
                    increment_message_received_pool(
                        &self.redis_pool,
                        api_key_id,
                        delivered_msg.content_size_bytes,
                        &self.config,
                    )
                    .await?;
                }
                messages.push(delivered_msg);

                // Queue immediate Redis delete + delayed SQL job (24h from delivered_at)
                let _ = self.delete_sender.try_send(DeleteTask {
                    msg_id: msg.id,
                    is_group_message: msg.is_group_message,
                });

                // Immediate Redis delete
                let _: () = conn.del(&redis_key).await?;
            } else {
                // Queue for SQL fallback
                fallback_ids.push(msg_id);
                warn!("Redis expired for {} - queuing SQL fallback", msg_id);
            }
        }

        // Clear recipient inbox
        let _: () = conn.del(&queue_key).await?;

        // SQL Fallback: Fetch undelivered messages for this recipient
        if !fallback_ids.is_empty() {
            let sql_msgs: Vec<Message> = query_as(
                "SELECT * FROM messages 
                 WHERE id = ANY($1) AND recipient_client_id = $2 AND is_delivered = false",
            )
            .bind(&fallback_ids)
            .bind(recipient_client_id)
            .fetch_all(&*self.db_pool)
            .await?;

            for mut msg in sql_msgs {
                // Verify envelope signature (using module-level Envelope)
                let envelope = Envelope {
                    id: &msg.id,
                    sender_client_id: &msg.sender_client_id,
                    recipient_client_id: &msg.recipient_client_id,
                    api_key_id: &msg.api_key_id,
                    is_group_message: msg.is_group_message,
                    content_size_bytes: msg.content_size_bytes,
                    created_at: &msg.created_at,
                    is_delivered: msg.is_delivered,
                    delivered_at: &msg.delivered_at,
                };
                let envelope_bytes = serde_json::to_vec(&envelope)?;

                if let Err(e) =
                    verify_signature(&envelope_bytes, &msg.signature, &msg.envelope_public_key)
                {
                    error!(
                        "SQL fallback verification failed for msg {}: {:?}",
                        msg.id, e
                    );
                    continue;
                }

                // Mark as delivered
                msg.is_delivered = true;
                msg.delivered_at = Some(Utc::now());

                // Metrics
                if let Some(api_key_id) = msg.api_key_id {
                    increment_message_received_pool(
                        &self.redis_pool,
                        api_key_id,
                        msg.content_size_bytes,
                        &self.config,
                    )
                    .await?;
                }

                let msg_id_for_delete = msg.id;
                let is_group_message_for_delete = msg.is_group_message;

                // Move `msg` into the vector.
                messages.push(msg);

                // Queue delayed delete job for SQL (24h from delivered_at)
                let _ = self.delete_sender.try_send(DeleteTask {
                    msg_id: msg_id_for_delete,
                    is_group_message: is_group_message_for_delete,
                });
            }
        }

        // Auto-mark as read
        for msg in &messages {
            let _ = self
                .mark_read_instant_message(recipient_client_id, msg.id)
                .await;
        }

        Ok(messages)
    }

    // -------------------------------------------------------------------------
    // Background flusher
    // -------------------------------------------------------------------------
    fn spawn_flusher(&self, mut rx: mpsc::Receiver<Message>) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(FLUSH_INTERVAL_SECS));
            let mut buffer: Vec<Message> = Vec::with_capacity(MAX_BATCH_SIZE);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !buffer.is_empty() {
                            if let Err(e) = flush_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!("Flush error: {:?}", e);
                            }
                        }
                    }
                    Some(msg) = rx.recv() => {
                        buffer.push(msg);
                        if buffer.len() >= MAX_BATCH_SIZE {
                            if let Err(e) = flush_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!("Immediate flush failed: {:?}", e);
                            }
                        }
                    }
                    else => break,
                }
            }

            if !buffer.is_empty() {
                let _ = flush_batch(&db_pool, &redis_pool, &mut buffer).await;
            }

            info!("InstantMessage flusher stopped");
        });
    }

    // -------------------------------------------------------------------------
    // Background deleter (immediate for Redis, delayed for SQL via purger)
    // -------------------------------------------------------------------------
    fn spawn_deleter(&self, mut rx: mpsc::Receiver<DeleteTask>) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
            let mut buffer: Vec<DeleteTask> = Vec::with_capacity(MAX_DELETE_BATCH);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !buffer.is_empty() {
                            if let Err(e) = delete_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!("Delete batch error: {:?}", e);
                            }
                        }
                    }
                    Some(task) = rx.recv() => {
                        buffer.push(task);
                        if buffer.len() >= MAX_DELETE_BATCH {
                            if let Err(e) = delete_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!("Immediate delete batch failed: {:?}", e);
                            }
                        }
                    }
                    else => break,
                }
            }

            if !buffer.is_empty() {
                let _ = delete_batch(&db_pool, &redis_pool, &mut buffer).await;
            }

            info!("InstantMessage deleter stopped");
        });
    }

    // -------------------------------------------------------------------------
    // Background purger for 24h delayed SQL deletes
    // -------------------------------------------------------------------------
    fn spawn_purger(&self) {
        let db_pool = Arc::clone(&self.db_pool);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(PURGE_INTERVAL_HOURS * 3600)); // Hourly check

            loop {
                ticker.tick().await;

                // Purge P2P messages delivered >24h ago
                let purged = sqlx::query(
                    r#"DELETE FROM messages 
                       WHERE is_delivered = true 
                       AND delivered_at IS NOT NULL 
                       AND delivered_at <= NOW() - make_interval(hours => $1)"#
                )
                .bind(RETENTION_AFTER_DELIVERY_HOURS)
                .execute(&*db_pool)
                .await
                .map(|res| res.rows_affected())
                .unwrap_or(0);

                // Also purge associated files/receipts if needed (cascade via FKs)
                info!("Purged {} old delivered messages", purged);
            }
        });
    }
}

// =============================================================================
// Batch Insert 
async fn flush_batch(
    db_pool: &PgPool,
    _redis_pool: &RedisPool, // Redis pool not needed in this fixed logic
    buffer: &mut Vec<Message>,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }

    // Always insert messages to enforce the 24h DB retention policy.
    // The Redis check is removed as it caused messages to be lost from the DB if fetched quickly.
    let to_insert = buffer.drain(..).collect::<Vec<Message>>();
    
    if to_insert.is_empty() {
        return Ok(());
    }

    let mut tx = db_pool.begin().await?;

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        r#"
        INSERT INTO messages (
            id, ciphertext, nonce, sender_client_id, recipient_client_id,
            api_key_id, is_group_message, content_size_bytes, created_at,
            is_delivered, delivered_at, signature, envelope_public_key, file_id
        ) 
        "#,
    );

    qb.push_values(to_insert.iter(), |mut b, msg| {
        b.push_bind(msg.id)
            .push_bind(&msg.ciphertext)
            .push_bind(&msg.nonce)
            .push_bind(msg.sender_client_id)
            .push_bind(msg.recipient_client_id)
            // Binding Option<T> directly
            .push_bind(msg.api_key_id)
            .push_bind(msg.is_group_message)
            .push_bind(msg.content_size_bytes)
            .push_bind(msg.created_at)
            .push_bind(msg.is_delivered)
            // Binding Option<T> directly
            .push_bind(msg.delivered_at)
            .push_bind(&msg.signature)
            .push_bind(&msg.envelope_public_key)
            // Binding Option<T> directly
            .push_bind(msg.file_id);
    });

    qb.push(" ON CONFLICT (id) DO NOTHING");

    let query = qb.build();
    let result = query.execute(&mut *tx).await?;
    tx.commit().await?;

    info!(
        "Flushed batch of {} messages to DB (affected: {})",
        to_insert.len(),
        result.rows_affected()
    );
    Ok(())
}

// =============================================================================
// Batch Delete (immediate for Redis/SQL, purger handles delay)
async fn delete_batch(
    db_pool: &PgPool,
    redis_pool: &RedisPool,
    buffer: &mut Vec<DeleteTask>,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }

    let mut rconn = redis_pool.get().await?;
    let mut tx = db_pool.begin().await?;

    let mut pipe = redis::pipe();
    let mut p2p_deletes = Vec::new();
    let mut group_updates = Vec::new();

    let task_count = buffer.len(); // Capture before drain

    for task in buffer.drain(..) {
        let redis_key = format!("msg:{}", task.msg_id);
        pipe.del(redis_key);

        if task.is_group_message {
            group_updates.push(task.msg_id);
        } else {
            p2p_deletes.push(task.msg_id);
        }
    }

    let _: () = pipe.query_async(&mut rconn).await?;

    if !p2p_deletes.is_empty() {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("DELETE FROM messages WHERE id = ANY (");
        qb.push_bind(p2p_deletes);
        qb.push(") AND is_group_message = false"); // Explicitly P2P delete
        let query = qb.build();
        let _ = query.execute(&mut *tx).await?;
    }

    if !group_updates.is_empty() {
        // Group updates only mark as delivered, relying on group cleanup for true delete
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            UPDATE messages 
            SET is_delivered = true, delivered_at = $1 
            WHERE id = ANY ($2) AND is_group_message = true AND delivered_at IS NULL
            "#,
        );
        qb.push_bind(Utc::now()).push_bind(group_updates);
        let query = qb.build();
        let _ = query.execute(&mut *tx).await?;
    }

    tx.commit().await?;
    info!("Deleted/updated batch of {} tasks", task_count);
    Ok(())
}