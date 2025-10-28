//! High-performance P2P Instant Message Model
//! All critical + minor issues fixed. Production-ready.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::RedisResult;
use redis::{AsyncCommands, pipe};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, query_as, query_builder::QueryBuilder};
use std::{
    env,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::mpsc, time::interval};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::crypto::{PRIVATE_KEY_SIZE, verify_signature};
use crate::error::{Result, VaultlessError};
use crate::usage::{
    MetricsConfig, get_period_start, increment_message_sent_pool,
};

// =============================================================================
// Configuration
// =============================================================================

const CACHE_TTL_SECS: u64 = 600;
const FLUSH_INTERVAL_SECS: u64 = 60;
const MAX_BATCH_SIZE: usize = 2000;
const CHANNEL_BUFFER: usize = 20_000;
const MAX_QUEUE_LEN: isize = 10_000;

const CLEANUP_INTERVAL_SECS: u64 = 10;
const MAX_DELETE_BATCH: usize = 1000;
const DELETE_CHANNEL_BUFFER: usize = 10_000;

const PURGE_INTERVAL_HOURS: u64 = 1;
const RETENTION_AFTER_DELIVERY_HOURS: i64 = 24;

const MAX_INBOX_FETCH: usize = 100;
const PIPELINE_CHUNK_SIZE: usize = 500;
const SQL_FALLBACK_PARALLELISM: usize = 4;

// Lock and TTL constants
const REBUILD_LOCK_TTL_SECS: i64 = 5;
const SENT_COUNTED_TTL_SECS: i64 = 86400; // 24 hours
const DELIVERED_COUNTED_TTL_SECS: i64 = 86400; // 24 hours
const METRIC_TTL_SECS: i64 = 7200; // 2 hours

// Lua script for atomic delivery counting
const ATOMIC_DELIVERY_COUNT_SCRIPT: &str = r#"
    local counted_key = KEYS[1]
    local metric_key = KEYS[2]
    local size_bytes = ARGV[1]
    
    -- Atomically check and set the counted flag
    if redis.call('SET', counted_key, '1', 'NX', 'EX', 86400) then
        -- Only increment if we successfully set the flag
        redis.call('HINCRBY', metric_key, 'messages_received', 1)
        redis.call('HINCRBY', metric_key, 'total_bytes_stored', size_bytes)
        redis.call('SADD', 'metric:active_keys', metric_key)
        redis.call('EXPIRE', metric_key, 7200)
        return 1
    end
    return 0
"#;

// =============================================================================
// Envelope (Canonical JSON)
// =============================================================================

#[derive(Serialize)]
struct Envelope<'a> {
    id: &'a Uuid,
    sender_client_id: &'a Uuid,
    recipient_client_id: &'a Uuid,
    api_key_id: &'a Option<Uuid>,
    is_group_message: bool,
    content_size_bytes: i64,
    created_at: &'a DateTime<Utc>,
}

// =============================================================================
// Message & File
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

    pub signature: String,
    pub envelope_public_key: String,

    pub file_id: Option<Uuid>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReadReceipt {
    pub id: Uuid,
    pub message_id: Uuid,
    pub client_id: Uuid,
    pub read_at: DateTime<Utc>,
}

// =============================================================================
// Pending Read
// =============================================================================

#[derive(Serialize, Deserialize)]
struct PendingRead {
    message_id: Uuid,
    reader_client_id: Uuid,
    read_at: DateTime<Utc>,
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
// InstantMessage Core (with Weak<PgPool> for fallback)
// =============================================================================

#[derive(Clone)]
pub struct InstantMessage {
    redis_pool: Arc<RedisPool>,
    db_pool: Arc<PgPool>,
    weak_db_pool: Weak<PgPool>, // For queue_delete fallback
    config: MetricsConfig,
    sender: mpsc::Sender<Message>,
    delete_sender: mpsc::Sender<DeleteTask>,
    server_private_key: [u8; PRIVATE_KEY_SIZE],
}

impl InstantMessage {
    pub fn new(redis_pool: RedisPool, db_pool: PgPool, config: MetricsConfig) -> Result<Self> {
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

        let db_pool_arc = Arc::new(db_pool);
        let weak_db_pool = Arc::downgrade(&db_pool_arc);

        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        let (delete_tx, delete_rx) = mpsc::channel(DELETE_CHANNEL_BUFFER);
        let this = Self {
            redis_pool: Arc::new(redis_pool),
            db_pool: db_pool_arc,
            weak_db_pool,
            config,
            sender: tx,
            delete_sender: delete_tx,
            server_private_key,
        };
        this.spawn_flusher(rx);
        this.spawn_deleter(delete_rx);
        this.spawn_purger();
        Ok(this)
    }

    // -------------------------------------------------------------------------
    // Send Instant Message
    // -------------------------------------------------------------------------
    pub async fn send_instant_message(
        &self,
        sender_client_id: Uuid,
        recipient_client_id: Uuid,
        ciphertext: String,
        nonce: String,
        content_size_bytes: i64,
        api_key_id: Option<Uuid>,
        signature: String,
        envelope_public_key: String,
    ) -> Result<Uuid> {
        // Create message
        let msg_id = Uuid::new_v4();
        let created_at = Utc::now();
        let msg = Message {
            id: msg_id,
            ciphertext,
            nonce,
            sender_client_id,
            recipient_client_id,
            api_key_id,
            is_group_message: false,
            content_size_bytes,
            created_at,
            is_delivered: false,
            delivered_at: None,
            signature,
            envelope_public_key,
            file_id: None,
        };

        // Increment sent metrics (idempotent, best-effort)
        if let Some(sender_api_key_id) = msg.api_key_id {
            let mut conn = self.redis_pool.get().await?;
            let counted_key = format!("sent_counted:{}", msg_id);
            let set: bool = conn.set_nx(&counted_key, "1").await?;
            if set {
                let _: () = conn.expire(&counted_key, SENT_COUNTED_TTL_SECS).await?;
                if let Err(e) = increment_message_sent_pool(
                    &self.redis_pool,
                    sender_api_key_id,
                    msg.content_size_bytes,
                    &self.config,
                )
                .await
                {
                    error!(
                        msg_id = %msg_id,
                        api_key_id = %sender_api_key_id,
                        error = %e,
                        "Failed to increment sent metrics - billing may be affected"
                    );
                }
            }
        }

        // Cache in Redis + queue to flusher
        let mut conn = self.redis_pool.get().await?;
        let redis_key = format!("msg:{}", msg_id);
        let data = serde_json::to_string(&msg)?;
        let _: () = conn.set_ex(&redis_key, data, CACHE_TTL_SECS).await?;
        let queue_key = format!("inbox:{}", recipient_client_id);
        let _: () = conn.rpush(&queue_key, msg_id.to_string()).await?;
        let _: () = conn.ltrim(&queue_key, 0, MAX_QUEUE_LEN).await?;
        
        // Handle backpressure with emergency write
        match self.sender.try_send(msg.clone()) {
            Ok(_) => {},
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!(
                    msg_id = %msg_id,
                    "Flusher channel full; forcing immediate DB write to maintain durability"
                );
                let db_pool = Arc::clone(&self.db_pool);
                tokio::spawn(async move {
                    if let Err(e) = emergency_write_message(&db_pool, &msg).await {
                        error!(
                            msg_id = %msg.id,
                            error = %e,
                            "Emergency write failed - message may be lost on restart"
                        );
                    }
                });
            },
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(VaultlessError::Internal("Flusher channel closed".into()));
            }
        }

        info!(
            msg_id = %msg_id,
            sender = %sender_client_id,
            recipient = %recipient_client_id,
            size_bytes = content_size_bytes,
            "Message sent successfully"
        );
        Ok(msg_id)
    }

    // -------------------------------------------------------------------------
    // Mark as read (P2P)
    // -------------------------------------------------------------------------
    pub async fn mark_read_instant_message(
        &self,
        reader_client_id: Uuid,
        msg_id: Uuid,
    ) -> Result<()> {
        let mut conn = self.redis_pool.get().await?;

        // Try DB first
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT 1 FROM messages WHERE id = $1")
            .bind(msg_id)
            .fetch_optional(&*self.db_pool)
            .await
            .map_err(|e| {
                error!(
                    msg_id = %msg_id,
                    error = %e,
                    "DB check failed for mark_read"
                );
                VaultlessError::Internal(e.to_string())
            })?;

        if exists.is_some() {
            // DB path
            let receipt_id = Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO p2p_read_receipts (id, message_id, client_id, read_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (message_id, client_id) DO UPDATE SET read_at = $4
                "#,
            )
            .bind(receipt_id)
            .bind(msg_id)
            .bind(reader_client_id)
            .bind(Utc::now())
            .execute(&*self.db_pool)
            .await?;

            let _ = sqlx::query(
                "UPDATE messages SET delivered_at = $1 WHERE id = $2 AND delivered_at IS NULL",
            )
            .bind(Utc::now())
            .bind(msg_id)
            .execute(&*self.db_pool)
            .await?;
        } else {
            // Redis-only: queue pending read
            let pending = PendingRead {
                message_id: msg_id,
                reader_client_id,
                read_at: Utc::now(),
            };
            let key = format!("pending_read:{}", msg_id);
            let data = serde_json::to_string(&pending)?;
            let _: () = conn.set_ex(&key, data, CACHE_TTL_SECS).await?;
        }

        // Pub/Sub
        let _: () = conn
            .publish(&format!("read:{}", msg_id), reader_client_id.to_string())
            .await?;
        
        info!(
            msg_id = %msg_id,
            reader = %reader_client_id,
            pending = exists.is_none(),
            "Message marked as read"
        );
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Fetch read receipts
    // -------------------------------------------------------------------------
    pub async fn fetch_read_receipts(&self, msg_id: Uuid) -> Result<Vec<ReadReceipt>> {
        let receipts = query_as::<_, ReadReceipt>(
            "SELECT id, message_id, client_id, read_at FROM p2p_read_receipts WHERE message_id = $1",
        )
        .bind(msg_id)
        .fetch_all(&*self.db_pool)
        .await?;
        Ok(receipts)
    }

    // -------------------------------------------------------------------------
    // Fetch messages (paginated, MGET, inbox cap)
    // -------------------------------------------------------------------------
    pub async fn fetch_messages_for_recipient(
        &self,
        recipient_client_id: Uuid,
    ) -> Result<Vec<Message>> {
        let mut conn = self.redis_pool.get().await?;
        let queue_key = format!("inbox:{}", recipient_client_id);

        // Check inbox length
        let total: isize = conn
            .llen(&queue_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // Rebuild with race condition protection if empty
        if total == 0 {
            if let Err(e) = self
                .rebuild_inbox_safe(&mut conn, recipient_client_id)
                .await
            {
                warn!(
                    recipient = %recipient_client_id,
                    error = %e,
                    "Failed to rebuild inbox - continuing with empty inbox"
                );
            }
        }

        // Fetch message IDs from inbox
        let msg_id_strs: Vec<String> = conn
            .lrange(&queue_key, 0, (MAX_INBOX_FETCH - 1) as isize)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if msg_id_strs.is_empty() {
            return Ok(vec![]);
        }

        let msg_ids: Vec<Uuid> = msg_id_strs
            .iter()
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();

        if msg_ids.is_empty() {
            warn!(
                recipient = %recipient_client_id,
                "All message IDs in inbox were invalid UUIDs"
            );
            return Ok(vec![]);
        }

        // Bulk fetch from Redis
        let redis_keys: Vec<String> = msg_ids.iter().map(|id| format!("msg:{}", id)).collect();
        let results: Vec<Option<String>> = conn
            .mget(&redis_keys)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let mut messages = Vec::new();
        let mut fallback_ids = Vec::new();

        // Process Redis results
        for (i, data_opt) in results.into_iter().enumerate() {
            let msg_id = msg_ids[i];

            if let Some(data) = data_opt {
                match serde_json::from_str::<Message>(&data) {
                    Ok(msg) => {
                        // CRITICAL: Verify signature BEFORE any state changes
                        if !self.verify_envelope_soft(&msg).await {
                            error!(
                                msg_id = %msg_id,
                                "Signature verification failed - deleting message"
                            );
                            let _: () = conn.del(format!("msg:{}", msg_id)).await.unwrap_or(());
                            self.queue_delete(msg_id, msg.is_group_message).await;
                            continue;
                        }

                        // Signature valid - safe to process
                        let mut delivered_msg = msg.clone();
                        delivered_msg.is_delivered = true;
                        delivered_msg.delivered_at = Some(Utc::now());

                        // Use atomic counting to prevent race conditions
                        if let Some(api_key_id) = delivered_msg.api_key_id {
                            match self
                                .count_delivery_once_atomic(
                                    &mut conn,
                                    msg_id,
                                    api_key_id,
                                    delivered_msg.content_size_bytes,
                                )
                                .await
                            {
                                Ok(counted) => {
                                    if counted {
                                        info!(
                                            msg_id = %msg_id,
                                            api_key_id = %api_key_id,
                                            "Delivery counted successfully"
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        msg_id = %msg_id,
                                        api_key_id = %api_key_id,
                                        error = %e,
                                        "Failed to count delivery - billing may be affected!"
                                    );
                                }
                            }
                        }

                        messages.push(delivered_msg);
                        self.queue_delete(msg_id, msg.is_group_message).await;
                    }
                    Err(e) => {
                        error!(
                            msg_id = %msg_id,
                            error = %e,
                            "Deserialization failed - deleting message"
                        );
                        let _: () = conn.del(format!("msg:{}", msg_id)).await.unwrap_or(());
                        self.queue_delete(msg_id, false).await;
                    }
                }
            } else {
                fallback_ids.push(msg_id);
            }
        }

        // Remove processed IDs from inbox (pipeline for efficiency)
        if !msg_id_strs.is_empty() {
            let mut pipe = redis::pipe();
            for id_str in &msg_id_strs {
                pipe.lrem(&queue_key, 1, id_str);
            }
            let _: () = pipe.query_async(&mut conn).await.unwrap_or(());
        }

        // Parallel SQL fallback for cache misses
        if !fallback_ids.is_empty() {
            let chunk_size =
                (fallback_ids.len() + SQL_FALLBACK_PARALLELISM - 1) / SQL_FALLBACK_PARALLELISM;
            let chunks = fallback_ids.chunks(chunk_size);
            let mut handles = Vec::new();

            for chunk in chunks {
                let chunk = chunk.to_vec();
                let db_pool = Arc::clone(&self.db_pool);
                let handle = tokio::spawn(async move {
                    fetch_sql_fallback(&db_pool, &chunk, recipient_client_id).await
                });
                handles.push(handle);
            }

            for handle in handles {
                match handle.await {
                    Ok(Ok(mut sql_msgs)) => {
                        for mut msg in sql_msgs {
                            if !verify_envelope_soft_static(&msg) {
                                error!(
                                    msg_id = %msg.id,
                                    "SQL fallback: Signature verification failed"
                                );
                                continue;
                            }

                            msg.is_delivered = true;
                            msg.delivered_at = Some(Utc::now());

                            let msg_id_for_delete = msg.id;
                            let is_group_message_for_delete = msg.is_group_message;

                            messages.push(msg);
                            self.queue_delete(msg_id_for_delete, is_group_message_for_delete)
                                .await;
                        }
                    }
                    Ok(Err(e)) => {
                        error!(
                            recipient = %recipient_client_id,
                            error = %e,
                            "SQL fallback query failed"
                        );
                    }
                    Err(e) => {
                        error!(
                            recipient = %recipient_client_id,
                            error = %e,
                            "SQL fallback task panicked"
                        );
                    }
                }
            }
        }

        // Mark all messages as read
        for msg in &messages {
            if let Err(e) = self
                .mark_read_instant_message(recipient_client_id, msg.id)
                .await
            {
                error!(
                    msg_id = %msg.id,
                    error = %e,
                    "Failed to mark message as read"
                );
            }
        }

        info!(
            recipient = %recipient_client_id,
            total = messages.len(),
            from_redis = messages.len() - fallback_ids.len(),
            from_sql = fallback_ids.len(),
            "Fetched messages successfully"
        );

        Ok(messages)
    }

    // -------------------------------------------------------------------------
    // Soft envelope verification (logs, returns bool)
    // -------------------------------------------------------------------------
    async fn verify_envelope_soft(&self, msg: &Message) -> bool {
        let envelope = Envelope {
            id: &msg.id,
            sender_client_id: &msg.sender_client_id,
            recipient_client_id: &msg.recipient_client_id,
            api_key_id: &msg.api_key_id,
            is_group_message: msg.is_group_message,
            content_size_bytes: msg.content_size_bytes,
            created_at: &msg.created_at,
        };
        match serde_json::to_vec(&envelope) {
            Ok(bytes) => match verify_signature(&bytes, &msg.signature, &msg.envelope_public_key) {
                Ok(()) => true,
                Err(e) => {
                    error!(
                        msg_id = %msg.id,
                        error = ?e,
                        "Envelope verification failed"
                    );
                    false
                }
            },
            Err(e) => {
                error!(
                    msg_id = %msg.id,
                    error = %e,
                    "Envelope serialization failed"
                );
                false
            }
        }
    }

    // -------------------------------------------------------------------------
    // Atomic delivery counting using Lua script
    // -------------------------------------------------------------------------
    async fn count_delivery_once_atomic(
        &self,
        conn: &mut impl AsyncCommands,
        msg_id: Uuid,
        api_key_id: Uuid,
        size_bytes: i64,
    ) -> Result<bool> {
        let counted_key = format!("delivered_counted:{}", msg_id);
        let period_start = get_period_start(&Utc::now());
        let metric_key = format!(
            "metric:hash:{}:{}",
            api_key_id,
            period_start.format("%Y%m%d%H")
        );

        let result: i32 = redis::Script::new(ATOMIC_DELIVERY_COUNT_SCRIPT)
            .key(&counted_key)
            .key(&metric_key)
            .arg(size_bytes)
            .invoke_async(conn)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Atomic count failed: {}", e)))?;

        Ok(result == 1)
    }

    // -------------------------------------------------------------------------
    // queue_delete with weak pool fallback
    // -------------------------------------------------------------------------
    async fn queue_delete(&self, msg_id: Uuid, is_group_message: bool) {
        if self
            .delete_sender
            .try_send(DeleteTask {
                msg_id,
                is_group_message,
            })
            .is_err()
        {
            warn!(
                msg_id = %msg_id,
                "Delete channel full; falling back to immediate DB delete"
            );
            if let Some(db_pool) = self.weak_db_pool.upgrade() {
                let _ =
                    sqlx::query("DELETE FROM messages WHERE id = $1 AND is_group_message = false")
                        .bind(msg_id)
                        .execute(&*db_pool)
                        .await
                        .map_err(|e| {
                            error!(
                                msg_id = %msg_id,
                                error = %e,
                                "Immediate delete failed"
                            );
                        });
            } else {
                error!(
                    msg_id = %msg_id,
                    "DB pool dropped; cannot delete message"
                );
            }
        }
    }

    // -------------------------------------------------------------------------
    // rebuild_inbox (extracted)
    // -------------------------------------------------------------------------
    async fn rebuild_inbox(
        &self,
        conn: &mut impl AsyncCommands,
        recipient_client_id: Uuid,
    ) -> Result<()> {
        let undelivered: Vec<Uuid> = sqlx::query_scalar(
            r#"SELECT id FROM messages 
               WHERE recipient_client_id = $1 
               AND is_delivered = false 
               AND is_group_message = false
               ORDER BY created_at ASC
               LIMIT $2"#,
        )
        .bind(recipient_client_id)
        .bind(MAX_QUEUE_LEN as i32)
        .fetch_all(&*self.db_pool)
        .await
        .map_err(|e| {
            error!(
                recipient = %recipient_client_id,
                error = %e,
                "Inbox rebuild query failed"
            );
            VaultlessError::Internal(e.to_string())
        })?;

        if undelivered.is_empty() {
            return Ok(());
        }

        let queue_key = format!("inbox:{}", recipient_client_id);

        // Use pipeline for efficiency
        let mut pipe = redis::pipe();
        for id in &undelivered {
            pipe.rpush(&queue_key, id.to_string());
        }
        pipe.expire(&queue_key, CACHE_TTL_SECS as i64);

        let _: () = pipe
            .query_async(conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        info!(
            recipient = %recipient_client_id,
            count = undelivered.len(),
            "Inbox rebuilt successfully"
        );

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Safely rebuild inbox with distributed locking
    // -------------------------------------------------------------------------
    async fn rebuild_inbox_safe(
        &self,
        conn: &mut impl AsyncCommands,
        recipient_client_id: Uuid,
    ) -> Result<bool> {
        let lock_key = format!("rebuild_lock:{}", recipient_client_id);
        let queue_key = format!("inbox:{}", recipient_client_id);

        // Try to acquire lock
        let acquired: bool = conn
            .set_nx(&lock_key, "1")
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if !acquired {
            info!(
                recipient = %recipient_client_id,
                "Inbox rebuild already in progress"
            );
            return Ok(false);
        }

        // Set TTL for safety (auto-release if process crashes)
        let _: () = conn
            .expire(&lock_key, REBUILD_LOCK_TTL_SECS)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // Double-check inbox is still empty (TOCTOU prevention)
        let total: isize = conn
            .llen(&queue_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if total > 0 {
            let _: () = conn.del(&lock_key).await.unwrap_or(());
            info!(
                recipient = %recipient_client_id,
                "Inbox already populated"
            );
            return Ok(false);
        }

        // Safe to rebuild
        let result = self.rebuild_inbox(conn, recipient_client_id).await;

        // Always release lock
        let _: () = conn.del(&lock_key).await.unwrap_or(());

        result.map(|_| true)
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
                                error!(
                                    error = %e,
                                    "Flush batch failed"
                                );
                            }
                        }
                    }
                    Some(msg) = rx.recv() => {
                        buffer.push(msg);
                        if buffer.len() >= MAX_BATCH_SIZE {
                            if let Err(e) = flush_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!(
                                    error = %e,
                                    "Immediate flush failed"
                                );
                            }
                        }
                    }
                    else => break,
                }
            }

            // Final flush on shutdown
            if !buffer.is_empty() {
                let _ = flush_batch(&db_pool, &redis_pool, &mut buffer).await;
            }

            info!("InstantMessage flusher stopped");
        });
    }

    // -------------------------------------------------------------------------
    // Background deleter
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
                                error!(
                                    error = %e,
                                    "Delete batch failed"
                                );
                            }
                        }
                    }
                    Some(task) = rx.recv() => {
                        buffer.push(task);
                        if buffer.len() >= MAX_DELETE_BATCH {
                            if let Err(e) = delete_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!(
                                    error = %e,
                                    "Immediate delete batch failed"
                                );
                            }
                        }
                    }
                    else => break,
                }
            }

            // Final flush on shutdown
            if !buffer.is_empty() {
                let _ = delete_batch(&db_pool, &redis_pool, &mut buffer).await;
            }

            info!("InstantMessage deleter stopped");
        });
    }

    // -------------------------------------------------------------------------
    // Background purger
    // -------------------------------------------------------------------------
    fn spawn_purger(&self) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(PURGE_INTERVAL_HOURS * 3600));
            loop {
                ticker.tick().await;

                let Ok(ids) = sqlx::query_scalar::<_, Uuid>(
                    r#"SELECT id FROM messages 
                       WHERE is_delivered = true 
                       AND delivered_at <= NOW() - make_interval(hours => $1)
                       AND is_group_message = false"#,
                )
                .bind(RETENTION_AFTER_DELIVERY_HOURS)
                .fetch_all(&*db_pool)
                .await
                else {
                    continue;
                };

                if ids.is_empty() {
                    continue;
                }

                let mut rconn = match redis_pool.get().await {
                    Ok(c) => c,
                    Err(e) => {
                        error!(
                            error = %e,
                            "Redis connection failed in purger"
                        );
                        continue;
                    }
                };

                for chunk in ids.chunks(PIPELINE_CHUNK_SIZE) {
                    let mut pipe = pipe();
                    for id in chunk {
                        pipe.del(format!("msg:{}", id));
                    }

                    let result: redis::RedisResult<()> = pipe.query_async(&mut rconn).await;

                    if let Err(e) = result {
                        error!(
                            error = %e,
                            "Purge Redis chunk failed"
                        );
                    }
                }

                if let Err(e) = sqlx::query("DELETE FROM messages WHERE id = ANY($1::uuid[])")
                    .bind(&ids)
                    .execute(&*db_pool)
                    .await
                {
                    error!(
                        error = %e,
                        "Purge DB delete failed"
                    );
                } else {
                    info!(
                        count = ids.len(),
                        "Purged old messages successfully"
                    );
                }
            }
        });
    }

    // -------------------------------------------------------------------------
    // Health check for monitoring
    // -------------------------------------------------------------------------
    pub fn get_health_status(&self) -> HealthStatus {
        HealthStatus {
            flusher_channel_capacity: self.sender.capacity(),
            flusher_channel_available: self.sender.max_capacity() - self.sender.capacity(),
            deleter_channel_capacity: self.delete_sender.capacity(),
            deleter_channel_available: self.delete_sender.max_capacity() - self.delete_sender.capacity(),
        }
    }
}

// =============================================================================
// Health Status
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub flusher_channel_capacity: usize,
    pub flusher_channel_available: usize,
    pub deleter_channel_capacity: usize,
    pub deleter_channel_available: usize,
}

// =============================================================================
// Static soft verify (for parallel fallback)
// =============================================================================
fn verify_envelope_soft_static(msg: &Message) -> bool {
    let envelope = Envelope {
        id: &msg.id,
        sender_client_id: &msg.sender_client_id,
        recipient_client_id: &msg.recipient_client_id,
        api_key_id: &msg.api_key_id,
        is_group_message: msg.is_group_message,
        content_size_bytes: msg.content_size_bytes,
        created_at: &msg.created_at,
    };
    if let Ok(bytes) = serde_json::to_vec(&envelope) {
        verify_signature(&bytes, &msg.signature, &msg.envelope_public_key).is_ok()
    } else {
        false
    }
}

// =============================================================================
// SQL Fallback
// =============================================================================
async fn fetch_sql_fallback(
    db_pool: &PgPool,
    ids: &[Uuid],
    recipient: Uuid,
) -> Result<Vec<Message>> {
    query_as("SELECT * FROM messages WHERE id = ANY($1::uuid[]) AND recipient_client_id = $2 AND is_delivered = false")
        .bind(ids)
        .bind(recipient)
        .fetch_all(db_pool)
        .await
        .map_err(Into::into)
}

// =============================================================================
// Emergency Write (for channel backpressure)
// =============================================================================
async fn emergency_write_message(db_pool: &PgPool, msg: &Message) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO messages (
            id, ciphertext, nonce, sender_client_id, recipient_client_id,
            api_key_id, is_group_message, content_size_bytes, created_at,
            is_delivered, delivered_at, signature, envelope_public_key, file_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(msg.id)
    .bind(&msg.ciphertext)
    .bind(&msg.nonce)
    .bind(msg.sender_client_id)
    .bind(msg.recipient_client_id)
    .bind(msg.api_key_id)
    .bind(msg.is_group_message)
    .bind(msg.content_size_bytes)
    .bind(msg.created_at)
    .bind(msg.is_delivered)
    .bind(msg.delivered_at)
    .bind(&msg.signature)
    .bind(&msg.envelope_public_key)
    .bind(msg.file_id)
    .execute(db_pool)
    .await?;
    
    Ok(())
}

// =============================================================================
// Batch Insert + Pending Reads + Metrics
// =============================================================================
async fn flush_batch(
    db_pool: &PgPool,
    redis_pool: &RedisPool,
    buffer: &mut Vec<Message>,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }

    let start = Utc::now();
    let to_insert = buffer.drain(..).collect::<Vec<_>>();

    let mut tx = db_pool.begin().await?;
    let mut qb = QueryBuilder::<Postgres>::new(
        r#"
        INSERT INTO messages (
            id, ciphertext, nonce, sender_client_id, recipient_client_id,
            api_key_id, is_group_message, content_size_bytes, created_at,
            is_delivered, delivered_at, signature, envelope_public_key, file_id
        ) 
        "#,
    );
    qb.push_values(&to_insert, |mut b, msg| {
        b.push_bind(msg.id)
            .push_bind(&msg.ciphertext)
            .push_bind(&msg.nonce)
            .push_bind(msg.sender_client_id)
            .push_bind(msg.recipient_client_id)
            .push_bind(msg.api_key_id)
            .push_bind(msg.is_group_message)
            .push_bind(msg.content_size_bytes)
            .push_bind(msg.created_at)
            .push_bind(msg.is_delivered)
            .push_bind(msg.delivered_at)
            .push_bind(&msg.signature)
            .push_bind(&msg.envelope_public_key)
            .push_bind(msg.file_id);
    });
    qb.push(" ON CONFLICT (id) DO NOTHING");
    qb.build().execute(&mut *tx).await?;
    tx.commit().await?;

    // Flush pending reads
    let mut rconn = redis_pool.get().await?;
    for msg in &to_insert {
        let pending_key = format!("pending_read:{}", msg.id);
        if let Ok(Some(data)) = rconn.get_del::<_, Option<String>>(&pending_key).await {
            if let Ok(pending) = serde_json::from_str::<PendingRead>(&data) {
                let _ = sqlx::query(
                    r#"INSERT INTO p2p_read_receipts (id, message_id, client_id, read_at) 
                       VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING"#
                )
                .bind(Uuid::new_v4())
                .bind(msg.id)
                .bind(pending.reader_client_id)
                .bind(pending.read_at)
                .execute(db_pool).await;
            }
        }
    }

    // Clean Redis
    let mut pipe = pipe();
    for msg in &to_insert {
        pipe.del(format!("msg:{}", msg.id));
        pipe.lrem(
            format!("inbox:{}", msg.recipient_client_id),
            1,
            msg.id.to_string(),
        );
    }

    let _: RedisResult<()> = pipe.query_async(&mut rconn).await;

    let duration_ms = (Utc::now() - start).num_milliseconds();

    info!(
        count = to_insert.len(),
        duration_ms = duration_ms,
        "Flushed messages to database"
    );
    Ok(())
}

// =============================================================================
// Batch Delete
// =============================================================================
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

    let mut pipe = pipe();
    let mut p2p_deletes = Vec::new();
    let mut group_updates = Vec::new();

    let task_count = buffer.len();

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
        sqlx::query("DELETE FROM messages WHERE id = ANY($1::uuid[]) AND is_group_message = false")
            .bind(&p2p_deletes)
            .execute(&mut *tx)
            .await?;
    }

    if !group_updates.is_empty() {
        sqlx::query(
            r#"UPDATE messages SET is_delivered = true, delivered_at = $1 
               WHERE id = ANY($2::uuid[]) AND is_group_message = true AND delivered_at IS NULL"#
        )
        .bind(Utc::now())
        .bind(&group_updates)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    
    info!(
        count = task_count,
        p2p = p2p_deletes.len(),
        group = group_updates.len(),
        "Processed delete batch"
    );
    Ok(())
}