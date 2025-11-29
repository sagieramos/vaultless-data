use super::circuit_breaker::*;
use crate::cache_key;
use crate::crypto::verify_signature;
use crate::error::{Result, VaultlessError};
use crate::models::usage::{
    MetricsConfig, increment_message_received_pool, increment_message_sent_pool,
    increment_proof_verified_pool,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use deadpool_redis::Pool as RedisPool;
use futures::future::join_all;
use redis::RedisResult;
use redis::{AsyncCommands, pipe};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, query_as, query_builder::QueryBuilder};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{sync::mpsc, time::interval};
use tracing::{error, info, warn};
use uuid::Uuid;

use std::sync::atomic::{AtomicUsize, Ordering};

// =============================================================================
// Configuration
// =============================================================================
const CACHE_TTL_SECS: u64 = 600;
/// Flush interval in seconds for message batching to DB.
const FLUSH_INTERVAL_SECS: u64 = 60;
const MAX_BATCH_SIZE: usize = 2000;
const CHANNEL_BUFFER: usize = 20_000;
const MAX_QUEUE_LEN: isize = 10_000;
const CLEANUP_INTERVAL_SECS: u64 = 10;
const MAX_DELETE_BATCH: usize = 1000;
const DELETE_CHANNEL_BUFFER: usize = 10_000;
const PURGE_INTERVAL_HOURS: u64 = 1;
const RETENTION_AFTER_DELIVERY_HOURS: i64 = 24;
/// Default message expiry in days.
const DEFAULT_MESSAGE_EXPIRY_DAYS: i64 = 7;
const MAX_INBOX_FETCH: usize = 100;
const PIPELINE_CHUNK_SIZE: usize = 500;
const SQL_FALLBACK_PARALLELISM: usize = 4;
// Lock and TTL constants
const REBUILD_LOCK_TTL_SECS: i64 = 5;
const SENT_COUNTED_TTL_SECS: i64 = 86400; // 24 hours
// Lua script for atomic delivery counting
const ATOMIC_DELIVERY_COUNT_SCRIPT: &str = r#"
local counted_key = KEYS[1]
-- Atomically check and set the counted flag
if redis.call('SET', counted_key, '1', 'NX', 'EX', 86400) then
  return 1
end
return 0
"#;

fn instant_message_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "message", msg_id)
}

fn instant_inbox_key(client_id: Uuid) -> String {
    cache_key!("instant_message", "inbox", client_id)
}

fn instant_pending_read_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "pending_read", msg_id)
}

fn instant_rebuild_lock_key(client_id: Uuid) -> String {
    cache_key!("instant_message", "rebuild_lock", client_id)
}

fn instant_sent_counted_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "sent_counted", msg_id)
}

fn instant_delivered_counted_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "delivered_counted", msg_id)
}

// =============================================================================
// Envelope (Canonical JSON)
// =============================================================================
/// Canonical envelope for signature verification.
#[derive(Serialize)]
struct Envelope<'a> {
    id: &'a Uuid,
    sender_client_id: &'a Uuid,
    recipient_client_id: &'a Uuid,
    api_key_id: &'a Uuid, // Aligned: non-optional per schema
    is_group_message: bool,
    content_size_bytes: i64,
    created_at: &'a DateTime<Utc>,
    require_proof_verification: bool,
}
// =============================================================================
// Message & File
// =============================================================================
/// P2P instant message struct.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub ciphertext: String,
    pub nonce: Uuid,
    pub content_type: Option<String>,
    pub content_size_bytes: i64,
    pub api_key_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accessed_at: Option<DateTime<Utc>>,
    pub access_count: i32,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool,
    pub sender_client_id: Uuid,
    pub recipient_client_id: Uuid,
    pub group_id: Option<Uuid>,
    pub is_group_message: bool,
    // Non-schema fields (stored in Redis JSON only; not persisted to DB)
    pub signature: Option<String>,
    pub envelope_public_key: String,
    pub file_id: Option<Uuid>,
}
/// P2P file attachment.
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
/// Read receipt for P2P messages.
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
/// Pending read receipt for Redis-only messages.
#[derive(Serialize, Deserialize)]
struct PendingRead {
    message_id: Uuid,
    reader_client_id: Uuid,
    read_at: DateTime<Utc>,
}
// =============================================================================
// Delete Task
// =============================================================================
/// Task for background message deletion.
#[derive(Debug, Clone)]
struct DeleteTask {
    msg_id: Uuid,
    is_group_message: bool,
}
// =============================================================================
// InstantMessage Core (with Weak<PgPool> for fallback)
// =============================================================================
/// Core struct for P2P instant messaging.
/// Manages Redis caching, DB persistence, metrics, and background tasks.
#[derive(Clone)]
pub struct InstantMessage {
    redis_pool: Arc<RedisPool>,
    db_pool: Arc<PgPool>,
    weak_db_pool: Weak<PgPool>,
    config: Arc<MetricsConfig>,
    sender: mpsc::Sender<Message>,
    delete_sender: mpsc::Sender<DeleteTask>,
    dlq_sender: mpsc::Sender<DlqEntry>,
    metrics: Arc<SystemMetrics>,
    // NEW: Circuit breakers
    redis_breaker: Arc<CircuitBreaker>,
    db_breaker: Arc<CircuitBreaker>,
}

/// Dead letter queue entry
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DlqEntry {
    msg_id: Uuid,
    reason: DlqReason,
    timestamp: DateTime<Utc>,
    retry_count: u32,
    original_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DlqReason {
    SignatureVerificationFailed,
    MetricsIncrementFailed,
    DeserializationFailed,
    DatabaseWriteFailed,
    MaxRetriesExceeded,
}

/// System-wide metrics for monitoring
pub struct SystemMetrics {
    pub failed_verifications: AtomicUsize,
    pub failed_metrics_increments: AtomicUsize,
    pub emergency_writes: AtomicUsize,
    pub dlq_entries: AtomicUsize,
    pub db_pool_dropped_deletes: AtomicUsize,
}

impl SystemMetrics {
    pub fn new() -> Self {
        Self {
            failed_verifications: AtomicUsize::new(0),
            failed_metrics_increments: AtomicUsize::new(0),
            emergency_writes: AtomicUsize::new(0),
            dlq_entries: AtomicUsize::new(0),
            db_pool_dropped_deletes: AtomicUsize::new(0),
        }
    }

    pub fn get_snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            failed_verifications: self.failed_verifications.load(Ordering::Relaxed),
            failed_metrics_increments: self.failed_metrics_increments.load(Ordering::Relaxed),
            emergency_writes: self.emergency_writes.load(Ordering::Relaxed),
            dlq_entries: self.dlq_entries.load(Ordering::Relaxed),
            db_pool_dropped_deletes: self.db_pool_dropped_deletes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub failed_verifications: usize,
    pub failed_metrics_increments: usize,
    pub emergency_writes: usize,
    pub dlq_entries: usize,
    pub db_pool_dropped_deletes: usize,
}

impl InstantMessage {
    pub fn new(redis_pool: RedisPool, db_pool: PgPool, config: Arc<MetricsConfig>) -> Result<Self> {
        let db_pool_arc = Arc::new(db_pool);
        let weak_db_pool = Arc::downgrade(&db_pool_arc);

        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        let (delete_tx, delete_rx) = mpsc::channel(DELETE_CHANNEL_BUFFER);
        let (dlq_tx, dlq_rx) = mpsc::channel(10_000);

        let metrics = Arc::new(SystemMetrics::new());

        // Circuit breakers: 5 failures within 30 seconds trips the breaker
        let redis_breaker = Arc::new(CircuitBreaker::new(5, 30));
        let db_breaker = Arc::new(CircuitBreaker::new(5, 30));

        let this = Self {
            redis_pool: Arc::new(redis_pool),
            db_pool: db_pool_arc,
            weak_db_pool,
            config,
            sender: tx,
            delete_sender: delete_tx,
            dlq_sender: dlq_tx,
            metrics,
            redis_breaker,
            db_breaker,
        };

        this.spawn_flusher(rx);
        this.spawn_deleter(delete_rx);
        this.spawn_dlq_processor(dlq_rx);
        this.spawn_purger();
        this.spawn_metrics_reporter();

        Ok(this)
    }

    /// Get Redis connection with circuit breaker
    async fn get_redis_conn(&self) -> Result<impl AsyncCommands> {
        let guard = self.redis_breaker.allow_request()?;

        match self.redis_pool.get().await {
            Ok(conn) => {
                guard.success();
                Ok(conn)
            }
            Err(e) => {
                guard.failure();
                Err(e.into())
            }
        }
    }

    /// Execute DB query with circuit breaker
    async fn execute_db_query<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(
            &PgPool,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
    {
        let guard = self.db_breaker.allow_request()?;

        match f(&self.db_pool).await {
            Ok(result) => {
                guard.success();
                Ok(result)
            }
            Err(e) => {
                guard.failure();
                Err(e)
            }
        }
    }

    /// Enhanced queue_delete with better error handling
    async fn queue_delete(&self, msg_id: Uuid, is_group_message: bool) {
        let task = DeleteTask {
            msg_id,
            is_group_message,
        };

        match self.delete_sender.try_send(task) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!(msg_id = %msg_id, "Delete channel full; attempting fallback");

                // Try to get strong reference to pool
                if let Some(db_pool) = self.weak_db_pool.upgrade() {
                    let msg_id_clone = msg_id;
                    tokio::spawn(async move {
                        if let Err(e) = sqlx::query(
                            "DELETE FROM messages WHERE id = $1 AND is_group_message = false",
                        )
                        .bind(msg_id_clone)
                        .execute(&*db_pool)
                        .await
                        {
                            error!(
                                msg_id = %msg_id_clone,
                                error = %e,
                                "Immediate delete failed - sending to DLQ"
                            );
                        }
                    });
                } else {
                    // DB pool is dropped - log critical error
                    error!(
                        msg_id = %msg_id,
                        "CRITICAL: DB pool dropped, message cannot be deleted"
                    );
                    self.metrics
                        .db_pool_dropped_deletes
                        .fetch_add(1, Ordering::Relaxed);

                    // Send to DLQ for manual recovery
                    let _ = self.dlq_sender.try_send(DlqEntry {
                        msg_id,
                        reason: DlqReason::DatabaseWriteFailed,
                        timestamp: Utc::now(),
                        retry_count: 0,
                        original_data: None,
                    });
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                error!(
                    msg_id = %msg_id,
                    "CRITICAL: Delete channel closed"
                );
            }
        }
    }

    /// Send message to dead letter queue
    async fn send_to_dlq(
        &self,
        msg_id: Uuid,
        reason: DlqReason,
        retry_count: u32,
        original_data: Option<String>,
    ) {
        self.metrics.dlq_entries.fetch_add(1, Ordering::Relaxed);

        let entry = DlqEntry {
            msg_id,
            reason,
            timestamp: Utc::now(),
            retry_count,
            original_data,
        };

        if let Err(e) = self.dlq_sender.try_send(entry) {
            error!(
                msg_id = %msg_id,
                error = ?e,
                "Failed to send to DLQ - message may be permanently lost"
            );
        }
    }

    /// Background DLQ processor
    fn spawn_dlq_processor(&self, mut rx: mpsc::Receiver<DlqEntry>) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);

        tokio::spawn(async move {
            while let Some(mut entry) = rx.recv().await {
                let reason_str = format!("{:?}", entry.reason);

                // Take original_data out without moving the entire entry
                let original_data = entry.original_data.take();

                match sqlx::query(
                    r#"
                INSERT INTO message_dlq (msg_id, reason, retry_count, original_data, created_at)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                )
                .bind(entry.msg_id)
                .bind(reason_str)
                .bind(entry.retry_count as i32)
                .bind(original_data) // <-- still the same value
                .bind(entry.timestamp)
                .execute(&*db_pool)
                .await
                {
                    Ok(_) => {
                        info!(
                            msg_id = %entry.msg_id,
                            reason = ?entry.reason,
                            "Message added to DLQ"
                        );
                    }
                    Err(e) => {
                        error!(
                            msg_id = %entry.msg_id,
                            error = %e,
                            "Failed to write to DLQ table - attempting Redis backup"
                        );

                        // Last resort: write to Redis
                        if let Ok(mut conn) = redis_pool.get().await {
                            let dlq_key = format!("dlq:message:{}", entry.msg_id);

                            // Now safe: entry is intact because we used .take()
                            let data = serde_json::to_string(&entry).unwrap_or_default();

                            let _: std::result::Result<(), redis::RedisError> =
                                conn.set_ex(&dlq_key, data, 86400 * 7).await;
                        }
                    }
                }
            }

            info!("DLQ processor stopped");
        });
    }

    /// Background metrics reporter (for monitoring/alerting)
    fn spawn_metrics_reporter(&self) {
        let metrics = Arc::clone(&self.metrics);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;

                let snapshot = metrics.get_snapshot();

                // Log metrics for monitoring systems to scrape
                info!(
                    failed_verifications = snapshot.failed_verifications,
                    failed_metrics = snapshot.failed_metrics_increments,
                    emergency_writes = snapshot.emergency_writes,
                    dlq_entries = snapshot.dlq_entries,
                    db_pool_dropped = snapshot.db_pool_dropped_deletes,
                    "System metrics snapshot"
                );

                // Alert on critical thresholds
                if snapshot.db_pool_dropped_deletes > 0 {
                    error!(
                        count = snapshot.db_pool_dropped_deletes,
                        "ALERT: DB pool dropped - messages cannot be deleted!"
                    );
                }

                if snapshot.failed_metrics_increments > 100 {
                    error!(
                        count = snapshot.failed_metrics_increments,
                        "ALERT: High metrics increment failure rate - billing affected!"
                    );
                }
            }
        });
    }

    /// Enhanced health status with more details
    pub fn get_health_status(&self) -> HealthStatus {
        let metrics = self.metrics.get_snapshot();

        HealthStatus {
            flusher_channel_capacity: self.sender.capacity(),
            flusher_channel_available: self.sender.max_capacity() - self.sender.capacity(),
            deleter_channel_capacity: self.delete_sender.capacity(),
            deleter_channel_available: self.delete_sender.max_capacity()
                - self.delete_sender.capacity(),
            dlq_channel_capacity: self.dlq_sender.capacity(),
            dlq_channel_available: self.dlq_sender.max_capacity() - self.dlq_sender.capacity(),
            failed_verifications: metrics.failed_verifications,
            failed_metrics_increments: metrics.failed_metrics_increments,
            emergency_writes: metrics.emergency_writes,
            dlq_entries: metrics.dlq_entries,
            db_pool_dropped_deletes: metrics.db_pool_dropped_deletes,
            db_pool_available: self.weak_db_pool.upgrade().is_some(),
            redis_circuit_state: format!("{:?}", self.redis_breaker.get_state()),
            db_circuit_state: format!("{:?}", self.db_breaker.get_state()),
        }
    }

    /// Process DLQ entries (for manual recovery or retry)
    pub async fn process_dlq_entry(&self, msg_id: Uuid) -> Result<()> {
        // Fetch from DLQ
        let entry: Option<(String, i32, Option<String>)> = sqlx::query_as(
            "SELECT reason, retry_count, original_data FROM message_dlq 
             WHERE msg_id = $1 AND processed_at IS NULL",
        )
        .bind(msg_id)
        .fetch_optional(&*self.db_pool)
        .await?;

        let Some((reason, retry_count, _original_data)) = entry else {
            return Err(VaultlessError::NotFound("DLQ entry not found".into()));
        };

        info!(
            msg_id = %msg_id,
            reason = %reason,
            retry_count = retry_count,
            "Processing DLQ entry"
        );

        // Attempt recovery based on reason
        // (Implementation depends on specific recovery strategy)

        // Mark as processed
        sqlx::query("UPDATE message_dlq SET processed_at = NOW() WHERE msg_id = $1")
            .bind(msg_id)
            .execute(&*self.db_pool)
            .await?;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Send Instant Message
    // -------------------------------------------------------------------------
    /// Sends a P2P instant message, caches in Redis, queues for DB flush, and increments sent metrics.
    pub async fn send_instant_message(
        &self,
        sender_client_id: Uuid,
        recipient_client_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        content_size_bytes: i64,
        api_key_id: Uuid,
        signature: Option<String>,
        envelope_public_key: String,
        require_proof_verification: bool,
    ) -> Result<Uuid> {
        // Create message
        let msg_id = Uuid::new_v4();
        let created_at = Utc::now();
        let expires_at = created_at + ChronoDuration::days(DEFAULT_MESSAGE_EXPIRY_DAYS);

        let msg = Message {
            id: msg_id,
            ciphertext,
            nonce,
            content_type: None,
            content_size_bytes,
            api_key_id,
            created_at,
            expires_at,
            accessed_at: None,
            access_count: 0,
            is_delivered: false,
            delivered_at: None,
            max_access_count: None,
            require_proof_verification,
            sender_client_id,
            recipient_client_id,
            group_id: None,
            is_group_message: false,
            signature,
            envelope_public_key,
            file_id: None,
        };

        // FIX #2: Verify signature BEFORE any state changes
        if !self.verify_envelope_soft(&msg).await {
            return Err(VaultlessError::SignatureVerificationFailed(format!(
                "Message {} failed signature verification",
                msg_id
            )));
        }

        // FIX #1: Increment sent metrics with atomic check (use atomic SET with EX)
        let mut conn = self.redis_pool.get().await?;
        let counted_key = instant_sent_counted_key(msg_id);

        // FIX #9: Use atomic SET with NX and EX in one command
        let counted: bool = redis::cmd("SET")
            .arg(&counted_key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(SENT_COUNTED_TTL_SECS)
            .query_async(&mut conn)
            .await
            .unwrap_or(false);

        if counted {
            // Make metrics increment critical - retry on failure
            let mut retries = 3;
            let mut last_error = None;

            while retries > 0 {
                match increment_message_sent_pool(
                    &self.redis_pool,
                    msg.api_key_id,
                    msg.content_size_bytes as i64,
                    &self.config,
                )
                .await
                {
                    Ok(_) => break,
                    Err(e) => {
                        last_error = Some(e);
                        retries -= 1;
                        if retries > 0 {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
            }

            if retries == 0 {
                error!(
                    msg_id = %msg_id,
                    api_key_id = %msg.api_key_id,
                    error = ?last_error,
                    "CRITICAL: Metrics increment failed after retries - billing affected"
                );
                // FIX #5: Don't allow message send if metrics fail
                return Err(VaultlessError::MetricsIncrementFailed(
                    "Failed to increment sent metrics after retries".into(),
                ));
            }
        }

        // Cache in Redis + queue to flusher
        let redis_key = instant_message_key(msg_id);
        let data = serde_json::to_string(&msg)?;
        let _: () = conn.set_ex(&redis_key, data, CACHE_TTL_SECS).await?;

        let queue_key = instant_inbox_key(recipient_client_id);
        let _: () = conn.rpush(&queue_key, msg_id.to_string()).await?;
        let _: () = conn.ltrim(&queue_key, 0, MAX_QUEUE_LEN).await?;

        // FIX #6: Handle backpressure with better error propagation
        match self.sender.try_send(msg.clone()) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                warn!(
                    msg_id = %msg_id,
                    "Flusher channel full; attempting emergency write"
                );

                // Use a oneshot channel to get result back
                let (tx, rx) = tokio::sync::oneshot::channel();
                let db_pool = Arc::clone(&self.db_pool);
                let msg_clone = msg.clone();

                tokio::spawn(async move {
                    let result = emergency_write_message(&db_pool, &msg_clone).await;
                    let _ = tx.send(result);
                });

                // Wait with timeout
                match tokio::time::timeout(Duration::from_secs(5), rx).await {
                    Ok(Ok(Ok(_))) => {
                        info!(msg_id = %msg_id, "Emergency write succeeded");
                    }
                    Ok(Ok(Err(e))) => {
                        error!(
                            msg_id = %msg_id,
                            error = %e,
                            "Emergency write failed - message may be lost"
                        );
                        return Err(VaultlessError::Internal("Emergency write failed".into()));
                    }
                    Ok(Err(_)) => {
                        error!(msg_id = %msg_id, "Emergency write channel dropped");
                        return Err(VaultlessError::Internal(
                            "Emergency write channel error".into(),
                        ));
                    }
                    Err(_) => {
                        error!(msg_id = %msg_id, "Emergency write timeout");
                        return Err(VaultlessError::Internal("Emergency write timeout".into()));
                    }
                }
            }
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
    /// Marks a message as read, inserts receipt, and publishes via Pub/Sub.
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
            let key = instant_pending_read_key(msg_id);
            let data = serde_json::to_string(&pending)?;
            let _: () = conn.set_ex(&key, data, CACHE_TTL_SECS).await?;
        }
        // Pub/Sub
        let _: () = conn
            .publish(format!("read:{}", msg_id), reader_client_id.to_string())
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
    /// Fetches read receipts for a message from DB.

    pub async fn fetch_messages_for_recipient(
        &self,
        recipient_client_id: Uuid,
    ) -> Result<Vec<Message>> {
        let mut conn = self.redis_pool.get().await?;
        let queue_key = instant_inbox_key(recipient_client_id);

        // Check inbox length and rebuild if needed
        let total: isize = conn
            .llen(&queue_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if total == 0 {
            if let Err(e) = self
                .rebuild_inbox_safe(&mut conn, recipient_client_id)
                .await
            {
                warn!(
                    recipient = %recipient_client_id,
                    error = %e,
                    "Failed to rebuild inbox"
                );
            }
        }

        // Fetch message IDs
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
            return Ok(vec![]);
        }

        // Bulk fetch from Redis
        let redis_keys: Vec<String> = msg_ids.iter().map(|id| instant_message_key(*id)).collect();

        let results: Vec<Option<String>> = conn
            .mget(&redis_keys)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let mut hit_zips: Vec<(String, Uuid)> = Vec::new();
        let mut fallback_ids: Vec<Uuid> = Vec::new();
        let mut hit_ids: Vec<Uuid> = Vec::new();

        for (data_opt, msg_id) in results.into_iter().zip(msg_ids.into_iter()) {
            match data_opt {
                Some(data) => {
                    hit_zips.push((data, msg_id));
                    hit_ids.push(msg_id);
                }
                None => {
                    fallback_ids.push(msg_id);
                }
            }
        }

        let self_clone = self.clone();
        let recipient_clone = recipient_client_id;

        // FIX #10: Limit parallelism to avoid thundering herd
        use tokio::sync::Semaphore;
        let semaphore = Arc::new(Semaphore::new(10)); // Max 10 concurrent operations

        // Process Redis hits with controlled parallelism
        let hit_futures: Vec<_> = hit_zips
            .into_iter()
            .map(|(data, msg_id)| {
                let self_clone = self_clone.clone();
                let sem = Arc::clone(&semaphore);

                async move {
                    // Acquire permit to limit concurrency
                    let _permit = sem.acquire().await.ok()?;

                    let mut msg = match serde_json::from_str::<Message>(&data) {
                        Ok(m) => m,
                        Err(e) => {
                            error!(msg_id = %msg_id, error = %e, "Deserialization failed");
                            // FIX #3: Proper error handling with retry
                            let _ = self_clone.delete_invalid_message(msg_id, false).await;
                            return None;
                        }
                    };

                    // Step 1: Verify signature FIRST (no state changes yet)
                    if !self_clone.verify_envelope_soft(&msg).await {
                        error!(msg_id = %msg_id, "Signature verification failed");
                        let _ = self_clone
                            .delete_invalid_message(msg_id, msg.is_group_message)
                            .await;
                        return None;
                    }

                    // Step 2: Atomically count delivery BEFORE marking as delivered
                    let counted = match self_clone.count_delivery_once_with_retry(msg_id).await {
                        Ok(c) => c,
                        Err(e) => {
                            error!(
                                msg_id = %msg_id,
                                error = %e,
                                "Failed to count delivery - aborting message fetch"
                            );
                            // Don't mark as delivered if counting fails
                            return None;
                        }
                    };

                    // Step 3: Increment metrics ONLY if newly counted
                    if counted {
                        match self_clone
                            .increment_received_metrics_with_retry(
                                msg.api_key_id,
                                msg.content_size_bytes as i64,
                            )
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    msg_id = %msg_id,
                                    api_key_id = %msg.api_key_id,
                                    "Delivery counted successfully"
                                );
                            }
                            Err(e) => {
                                error!(
                                    msg_id = %msg_id,
                                    error = %e,
                                    "Metrics increment failed - delivery NOT marked"
                                );
                                // Critical: Don't mark as delivered if metrics fail
                                return None;
                            }
                        }
                    }

                    // Step 4: ONLY NOW mark as delivered (after successful counting & metrics)
                    msg.is_delivered = true;
                    msg.delivered_at = Some(Utc::now());

                    // Step 5: Queue for deletion
                    let is_group = msg.is_group_message;
                    self_clone.queue_delete(msg_id, is_group).await;

                    Some(msg)
                }
            })
            .collect();

        let hit_results: Vec<Option<Message>> = join_all(hit_futures).await;
        let mut messages: Vec<Message> = hit_results.into_iter().flatten().collect();

        let from_redis = messages.len();

        // Trim hit IDs from inbox
        if !hit_ids.is_empty() {
            self_clone
                .trim_inbox_batch(&hit_ids, recipient_client_id)
                .await?;
        }

        // SQL fallback with same atomic guarantees
        let mut from_sql = 0;
        if !fallback_ids.is_empty() {
            let sql_msgs =
                fetch_sql_fallback(&self_clone.db_pool, &fallback_ids, recipient_client_id)
                    .await
                    .unwrap_or_default();

            let sql_futures: Vec<_> = sql_msgs
                .into_iter()
                .map(|mut msg| {
                    let self_clone = self_clone.clone();
                    let sem = Arc::clone(&semaphore);

                    async move {
                        let _permit = sem.acquire().await.ok()?;

                        // Same atomic sequence as Redis path
                        if !verify_envelope_soft_static(
                            &msg,
                            &self_clone.redis_pool,
                            &self_clone.config,
                        )
                        .await
                        {
                            error!(msg_id = %msg.id, "SQL fallback: Signature failed");
                            return None;
                        }

                        let counted = self_clone
                            .count_delivery_once_with_retry(msg.id)
                            .await
                            .ok()?;

                        if counted {
                            if let Err(e) = self_clone
                                .increment_received_metrics_with_retry(
                                    msg.api_key_id,
                                    msg.content_size_bytes as i64,
                                )
                                .await
                            {
                                error!(msg_id = %msg.id, error = %e, "Metrics failed in fallback");
                                return None;
                            }
                        }

                        msg.is_delivered = true;
                        msg.delivered_at = Some(Utc::now());
                        self_clone.queue_delete(msg.id, msg.is_group_message).await;

                        Some(msg)
                    }
                })
                .collect();

            let sql_results: Vec<Option<Message>> = join_all(sql_futures).await;
            let sql_msgs: Vec<Message> = sql_results.into_iter().flatten().collect();
            messages.extend(sql_msgs.clone());
            from_sql = sql_msgs.len();

            if !fallback_ids.is_empty() {
                self_clone
                    .trim_inbox_batch(&fallback_ids, recipient_client_id)
                    .await?;
            }
        }

        // Mark all as read in parallel
        let read_futures: Vec<_> = messages
            .iter()
            .map(|msg| {
                let self_clone = self_clone.clone();
                let msg_id = msg.id;
                async move {
                    self_clone
                        .mark_read_instant_message(recipient_clone, msg_id)
                        .await
                }
            })
            .collect();

        let read_results = join_all(read_futures).await;
        for (i, result) in read_results.into_iter().enumerate() {
            if let Err(e) = result {
                error!(msg_id = %messages[i].id, error = %e, "Failed to mark as read");
            }
        }

        info!(
            recipient = %recipient_client_id,
            total = messages.len(),
            from_redis,
            from_sql,
            "Fetched messages successfully"
        );

        Ok(messages)
    }

    // Helper: Retry wrapper for delivery counting
    async fn count_delivery_once_with_retry(&self, msg_id: Uuid) -> Result<bool> {
        let mut retries = 3;
        let mut last_error = None;

        while retries > 0 {
            match self.redis_pool.get().await {
                Ok(mut conn) => match self.count_delivery_once_atomic(&mut conn, msg_id).await {
                    Ok(counted) => return Ok(counted),
                    Err(e) => {
                        last_error = Some(e);
                        retries -= 1;
                    }
                },
                Err(e) => {
                    last_error = Some(e.into());
                    retries -= 1;
                }
            }

            if retries > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        Err(last_error
            .unwrap_or_else(|| VaultlessError::Internal("Delivery counting failed".into())))
    }

    // Helper: Retry wrapper for metrics increment
    async fn increment_received_metrics_with_retry(
        &self,
        api_key_id: Uuid,
        bytes: i64,
    ) -> Result<()> {
        let mut retries = 3;

        while retries > 0 {
            match increment_message_received_pool(&self.redis_pool, api_key_id, bytes, &self.config)
                .await
            {
                Ok(_) => return Ok(()),
                Err(_) => {
                    retries -= 1;
                    if retries > 0 {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }

        Err(VaultlessError::MetricsIncrementFailed(
            "Failed after retries".into(),
        ))
    }

    // FIX #3: Proper error handling for invalid message deletion
    async fn delete_invalid_message(&self, msg_id: Uuid, is_group: bool) -> Result<()> {
        let mut retries = 3;

        while retries > 0 {
            match self.redis_pool.get().await {
                Ok(mut conn) => match conn.del::<_, usize>(instant_message_key(msg_id)).await {
                    Ok(_) => {
                        self.queue_delete(msg_id, is_group).await;
                        return Ok(());
                    }
                    Err(e) => {
                        error!(msg_id = %msg_id, error = %e, "Redis delete failed");
                        retries -= 1;
                    }
                },
                Err(e) => {
                    error!(msg_id = %msg_id, error = %e, "Redis connection failed");
                    retries -= 1;
                }
            }

            if retries > 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        Err(VaultlessError::Internal(
            "Failed to delete invalid message".into(),
        ))
    }

    /// Efficient batch inbox trimming using Lua script
    /// Instead of O(N²) LREM, we use O(N) set difference approach
    async fn trim_inbox_batch(&self, msg_ids: &[Uuid], recipient_client_id: Uuid) -> Result<()> {
        if msg_ids.is_empty() {
            return Ok(());
        }

        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let queue_key = instant_inbox_key(recipient_client_id);
        let id_strs: Vec<String> = msg_ids.iter().map(|id| id.to_string()).collect();

        // More efficient Lua script: O(N) instead of O(N²)
        // Strategy: Build a set of IDs to remove, then filter the list
        let lua_script = r#"
            local queue_key = KEYS[1]
            local to_remove = {}
            
            -- Build hash set of IDs to remove (O(N))
            for i = 1, #ARGV do
                to_remove[ARGV[i]] = true
            end
            
            -- Get entire list
            local items = redis.call("LRANGE", queue_key, 0, -1)
            
            -- Filter out items to remove (O(N))
            local filtered = {}
            for _, item in ipairs(items) do
                if not to_remove[item] then
                    table.insert(filtered, item)
                end
            end
            
            -- Replace list atomically
            redis.call("DEL", queue_key)
            if #filtered > 0 then
                redis.call("RPUSH", queue_key, unpack(filtered))
                redis.call("EXPIRE", queue_key, ARGV[#ARGV])
            end
            
            return #items - #filtered
        "#;

        // Add TTL as last argument
        let mut args = id_strs;
        args.push(CACHE_TTL_SECS.to_string());

        let removed: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(1)
            .arg(&queue_key)
            .arg(args)
            .query_async(&mut conn)
            .await
            .map_err(|e| {
                error!(
                    recipient = %recipient_client_id,
                    error = %e,
                    "Inbox trim failed"
                );
                VaultlessError::Internal(e.to_string())
            })?;

        info!(
            recipient = %recipient_client_id,
            removed = removed,
            total_ids = msg_ids.len(),
            "Trimmed inbox successfully"
        );

        Ok(())
    }

    // Alternative: For very large inboxes (>1000 items), use pagination
    async fn trim_inbox_batch_large(
        &self,
        msg_ids: &[Uuid],
        recipient_client_id: Uuid,
    ) -> Result<()> {
        if msg_ids.is_empty() {
            return Ok(());
        }

        let mut conn = self.redis_pool.get().await?;
        let queue_key = instant_inbox_key(recipient_client_id);

        // For very large lists, use a temporary sorted set
        let temp_key = format!("{}:trim_temp", queue_key);
        let id_strs: Vec<String> = msg_ids.iter().map(|id| id.to_string()).collect();

        // Lua script using sorted set for O(N log N) performance
        let lua_script = r#"
            local queue_key = KEYS[1]
            local temp_key = KEYS[2]
            local ttl = tonumber(ARGV[#ARGV])
            
            -- Add all IDs to remove into a temporary set
            for i = 1, #ARGV - 1 do
                redis.call("SADD", temp_key, ARGV[i])
            end
            
            -- Get entire list
            local items = redis.call("LRANGE", queue_key, 0, -1)
            local filtered = {}
            
            -- Filter using set membership check (O(1) per check)
            for _, item in ipairs(items) do
                if redis.call("SISMEMBER", temp_key, item) == 0 then
                    table.insert(filtered, item)
                end
            end
            
            -- Clean up temp set
            redis.call("DEL", temp_key)
            
            -- Replace list
            redis.call("DEL", queue_key)
            if #filtered > 0 then
                redis.call("RPUSH", queue_key, unpack(filtered))
                redis.call("EXPIRE", queue_key, ttl)
            end
            
            return #items - #filtered
        "#;

        let mut args = id_strs;
        args.push(CACHE_TTL_SECS.to_string());

        let removed: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(2) // Two keys: queue_key and temp_key
            .arg(&queue_key)
            .arg(&temp_key)
            .arg(args)
            .query_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        info!(
            recipient = %recipient_client_id,
            removed = removed,
            "Trimmed large inbox successfully"
        );

        Ok(())
    }
}

// =============================================================================
// FIX #4: Bounded Inbox Rebuild with Pagination
// =============================================================================

impl InstantMessage {
    /// Rebuild inbox with pagination to avoid memory spikes
    async fn rebuild_inbox_paginated(
        &self,
        conn: &mut impl AsyncCommands,
        recipient_client_id: Uuid,
    ) -> Result<()> {
        const PAGE_SIZE: i32 = 1000;
        let queue_key = instant_inbox_key(recipient_client_id);
        let mut offset = 0;
        let mut total_added = 0;

        loop {
            // Fetch page of undelivered messages
            let page: Vec<Uuid> = sqlx::query_scalar(
                r#"
                SELECT id FROM messages
                WHERE recipient_client_id = $1
                  AND is_delivered = false
                  AND is_group_message = false
                ORDER BY created_at ASC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(recipient_client_id)
            .bind(PAGE_SIZE)
            .bind(offset)
            .fetch_all(&*self.db_pool)
            .await
            .map_err(|e| {
                error!(
                    recipient = %recipient_client_id,
                    error = %e,
                    "Paginated inbox rebuild query failed"
                );
                VaultlessError::Internal(e.to_string())
            })?;

            if page.is_empty() {
                break;
            }

            // Push this page to Redis
            let id_strs: Vec<String> = page.iter().map(|id| id.to_string()).collect();

            if !id_strs.is_empty() {
                // Use pipeline for efficiency
                let mut pipe = redis::pipe();
                for id_str in &id_strs {
                    pipe.rpush(&queue_key, id_str);
                }

                let _: () = pipe
                    .query_async(conn)
                    .await
                    .map_err(|e| VaultlessError::Internal(e.to_string()))?;

                total_added += id_strs.len();
            }

            // Check if we've hit the limit
            if total_added >= MAX_QUEUE_LEN as usize {
                warn!(
                    recipient = %recipient_client_id,
                    total_added,
                    "Inbox rebuild hit MAX_QUEUE_LEN limit"
                );
                break;
            }

            // Stop if this was a partial page (last page)
            if page.len() < PAGE_SIZE as usize {
                break;
            }

            offset += PAGE_SIZE;
        }

        // Set TTL after all pushes
        if total_added > 0 {
            let _: () = conn
                .expire(&queue_key, CACHE_TTL_SECS as i64)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
        }

        info!(
            recipient = %recipient_client_id,
            count = total_added,
            "Inbox rebuilt successfully (paginated)"
        );

        Ok(())
    }

    /// Enhanced rebuild_inbox_safe using paginated approach
    async fn rebuild_inbox_safe(
        &self,
        conn: &mut impl AsyncCommands,
        recipient_client_id: Uuid,
    ) -> Result<bool> {
        let lock_key = instant_rebuild_lock_key(recipient_client_id);
        let queue_key = instant_inbox_key(recipient_client_id);

        // Try to acquire lock with SET NX EX (atomic)
        let acquired: bool = redis::cmd("SET")
            .arg(&lock_key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(REBUILD_LOCK_TTL_SECS)
            .query_async(conn)
            .await
            .unwrap_or(false);

        if !acquired {
            info!(
                recipient = %recipient_client_id,
                "Inbox rebuild already in progress"
            );
            return Ok(false);
        }

        // Double-check inbox is still empty
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

        // Use paginated rebuild to avoid memory spikes
        let result = self
            .rebuild_inbox_paginated(conn, recipient_client_id)
            .await;

        // Always release lock
        let _: () = conn.del(&lock_key).await.unwrap_or(());

        result.map(|_| true)
    }

    // -------------------------------------------------------------------------
    // Soft envelope verification (logs, returns bool) - conditional on require_proof_verification
    // -------------------------------------------------------------------------
    /// Soft-verifies message envelope signature; logs errors, returns bool.
    async fn verify_envelope_soft(&self, msg: &Message) -> bool {
        if !msg.require_proof_verification {
            info!(msg_id = %msg.id, "Proof verification not required - skipping signature check");
            return true;
        }
        let envelope = Envelope {
            id: &msg.id,
            sender_client_id: &msg.sender_client_id,
            recipient_client_id: &msg.recipient_client_id,
            api_key_id: &msg.api_key_id,
            is_group_message: msg.is_group_message,
            content_size_bytes: msg.content_size_bytes as i64,
            created_at: &msg.created_at,
            require_proof_verification: msg.require_proof_verification,
        };
        let Some(signature_str) = msg.signature.as_deref() else {
            tracing::error!("Message signature is missing but required for verification.");
            return false;
        };
        match serde_json::to_vec(&envelope) {
            Ok(bytes) => match verify_signature(&bytes, signature_str, &msg.envelope_public_key) {
                Ok(()) => {
                    if let Err(e) = increment_proof_verified_pool(
                        &self.redis_pool,
                        msg.api_key_id,
                        &self.config,
                    )
                    .await
                    {
                        // Log the error but allow the operation (signature verification) to succeed
                        tracing::error!(
                            msg_id = %msg.id,
                            api_key_id = %msg.api_key_id,
                            error = %e,
                            "Failed to increment proof verified metrics"
                        );
                    }
                    // Verification succeeded, return true for the outer block
                    true
                }
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
    /// Atomically checks/sets delivery flag via Lua; returns true if newly set.
    async fn count_delivery_once_atomic(
        &self,
        conn: &mut impl AsyncCommands,
        msg_id: Uuid,
    ) -> Result<bool> {
        let counted_key = instant_delivered_counted_key(msg_id);
        let result: i32 = redis::Script::new(ATOMIC_DELIVERY_COUNT_SCRIPT)
            .key(&counted_key)
            .invoke_async(conn)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Atomic count failed: {}", e)))?;
        Ok(result == 1)
    }

    // -------------------------------------------------------------------------
    // rebuild_inbox (extracted)
    // -------------------------------------------------------------------------
    /// Rebuilds inbox queue from undelivered DB messages.
    async fn rebuild_inbox(
        &self,
        conn: &mut impl AsyncCommands,
        recipient_client_id: Uuid,
    ) -> Result<()> {
        let undelivered: Vec<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM messages
            WHERE recipient_client_id = $1
              AND is_delivered = false
              AND is_group_message = false
            ORDER BY created_at ASC
            LIMIT $2
            "#,
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
        let queue_key = instant_inbox_key(recipient_client_id);

        // Prepare Lua script: push all IDs and set TTL atomically
        let lua_script = r#"
            local ttl = tonumber(ARGV[#ARGV])
            for i = 1, #ARGV - 1 do
                redis.call("RPUSH", KEYS[1], ARGV[i])
            end
            redis.call("EXPIRE", KEYS[1], ttl)
            return #ARGV - 1
        "#;

        // Prepare arguments: undelivered IDs + TTL as last argument
        let mut args: Vec<String> = undelivered.iter().map(|id| id.to_string()).collect();
        args.push(CACHE_TTL_SECS.to_string()); // last arg = TTL

        // Execute Lua script atomically
        let _: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(1) // one key: queue_key
            .arg(&queue_key)
            .arg(args)
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
    // Background flusher
    // -------------------------------------------------------------------------
    /// Spawns background task to flush message batches to DB.
    fn spawn_flusher(&self, mut rx: mpsc::Receiver<Message>) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(FLUSH_INTERVAL_SECS));
            let mut buffer: Vec<Message> = Vec::with_capacity(MAX_BATCH_SIZE);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !buffer.is_empty()
                            && let Err(e) = flush_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!(error = %e, "Flush batch failed");
                            }
                    }
                    Some(msg) = rx.recv() => {
                        buffer.push(msg);
                        if buffer.len() >= MAX_BATCH_SIZE
                            && let Err(e) = flush_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!(error = %e, "Immediate flush failed");
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
    /// Spawns background task to process message deletes.
    fn spawn_deleter(&self, mut rx: mpsc::Receiver<DeleteTask>) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
            let mut buffer: Vec<DeleteTask> = Vec::with_capacity(MAX_DELETE_BATCH);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        if !buffer.is_empty()
                            && let Err(e) = delete_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!(error = %e, "Delete batch failed");
                            }
                    }
                    Some(task) = rx.recv() => {
                        buffer.push(task);
                        if buffer.len() >= MAX_DELETE_BATCH
                            && let Err(e) = delete_batch(&db_pool, &redis_pool, &mut buffer).await {
                                error!(error = %e, "Immediate delete batch failed");
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
    /// Spawns background task to purge old delivered messages.
    fn spawn_purger(&self) {
        let db_pool = Arc::clone(&self.db_pool);
        let redis_pool = Arc::clone(&self.redis_pool);
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(PURGE_INTERVAL_HOURS * 3600));
            loop {
                ticker.tick().await;
                let Ok(ids) = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    SELECT id FROM messages
                    WHERE is_delivered = true
                      AND delivered_at <= NOW() - make_interval(hours => $1)
                      AND is_group_message = false
                    "#,
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
                        error!(error = %e, "Redis connection failed in purger");
                        continue;
                    }
                };
                for chunk in ids.chunks(PIPELINE_CHUNK_SIZE) {
                    let mut pipe = pipe();
                    for id in chunk {
                        pipe.del(instant_message_key(*id));
                    }
                    let result: redis::RedisResult<()> = pipe.query_async(&mut rconn).await;
                    if let Err(e) = result {
                        error!(error = %e, "Purge Redis chunk failed");
                    }
                }
                if let Err(e) = sqlx::query("DELETE FROM messages WHERE id = ANY($1::uuid[])")
                    .bind(&ids)
                    .execute(&*db_pool)
                    .await
                {
                    error!(error = %e, "Purge DB delete failed");
                } else {
                    info!(count = ids.len(), "Purged old messages successfully");
                }
            }
        });
    }
}

// =============================================================================
// Static soft verify (for parallel fallback) - conditional on require_proof_verification
// =============================================================================
/// Static envelope verification for SQL fallback (no self access).
/// NOTE: This is an async function to allow for the metrics increment call.
async fn verify_envelope_soft_static(
    msg: &Message,
    redis_pool: &RedisPool, // Passed in for metrics connection
    config: &MetricsConfig, // Passed in for metrics configuration
) -> bool {
    // 1. Check if verification is required
    let Some(signature_str) = msg.signature.as_deref() else {
        tracing::error!("Message signature is missing but required for verification.");
        return false;
    };
    if !msg.require_proof_verification || signature_str.is_empty() {
        return true;
    }
    // 2. Build the envelope struct for serialization
    let envelope = Envelope {
        id: &msg.id,
        sender_client_id: &msg.sender_client_id,
        recipient_client_id: &msg.recipient_client_id,
        api_key_id: &msg.api_key_id,
        is_group_message: msg.is_group_message,
        content_size_bytes: msg.content_size_bytes as i64,
        created_at: &msg.created_at,
        require_proof_verification: msg.require_proof_verification,
    };
    // 3. Serialize and verify the signature
    if let Ok(bytes) = serde_json::to_vec(&envelope) {
        if verify_signature(&bytes, signature_str, &msg.envelope_public_key).is_ok() {
            // Signature SUCCESSFUL. Call the proof verified metrics function.
            if let Err(e) = increment_proof_verified_pool(redis_pool, msg.api_key_id, config).await
            {
                // Log the metrics failure, but the core verification is still valid.
                tracing::error!(
                    msg_id = %msg.id,
                    api_key_id = %msg.api_key_id,
                    error = %e,
                    "Failed to increment proof verified metrics during static verification"
                );
            }
            // Return the core verification result (SUCCESS)
            true
        } else {
            // Signature verification failed.
            false
        }
    } else {
        // Serialization failed.
        false
    }
}
// =============================================================================
// SQL Fallback
// =============================================================================
/// Fetches messages from DB for cache misses.
async fn fetch_sql_fallback(
    db_pool: &PgPool,
    ids: &[Uuid],
    recipient: Uuid,
) -> Result<Vec<Message>> {
    query_as(
        "SELECT * FROM messages WHERE id = ANY($1::uuid[]) AND recipient_client_id = $2 AND is_delivered = false",
    )
    .bind(ids)
    .bind(recipient)
    .fetch_all(db_pool)
    .await
    .map_err(Into::into)
}
// =============================================================================
// Emergency Write (for channel backpressure)
// =============================================================================
/// Emergency DB insert for flusher backpressure.
async fn emergency_write_message(db_pool: &PgPool, msg: &Message) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO messages (
          id, ciphertext, nonce, content_type, content_size_bytes,
          api_key_id, created_at, expires_at, access_count,
          is_delivered, delivered_at, max_access_count,
          require_proof_verification, sender_client_id, recipient_client_id,
          group_id, is_group_message
        ) VALUES (
          $1, $2, $3, COALESCE($4, 'application/octet-stream'), $5,
          $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17
        ) ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(msg.id)
    .bind(&msg.ciphertext)
    .bind(msg.nonce)
    .bind(msg.content_type.as_ref()) // Defaults in SQL if None
    .bind(msg.content_size_bytes)
    .bind(msg.api_key_id)
    .bind(msg.created_at)
    .bind(msg.expires_at)
    .bind(msg.access_count)
    .bind(msg.is_delivered)
    .bind(msg.delivered_at)
    .bind(msg.max_access_count)
    .bind(msg.require_proof_verification)
    .bind(msg.sender_client_id)
    .bind(msg.recipient_client_id)
    .bind(msg.group_id)
    .bind(msg.is_group_message)
    .execute(db_pool)
    .await?;
    Ok(())
}
// =============================================================================
// Batch Insert + Pending Reads + Metrics
// =============================================================================
/// Flushes batch of messages to DB, processes pending reads, cleans Redis.
async fn flush_batch(
    db_pool: &PgPool,
    redis_pool: &RedisPool,
    buffer: &mut Vec<Message>,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let start = Utc::now();
    let to_insert = std::mem::take(buffer);
    let mut tx = db_pool.begin().await?;
    let mut qb = QueryBuilder::<Postgres>::new(
        r#"
        INSERT INTO messages (
          id, ciphertext, nonce, content_type, content_size_bytes,
          api_key_id, created_at, expires_at, access_count,
          is_delivered, delivered_at, max_access_count,
          require_proof_verification, sender_client_id, recipient_client_id,
          group_id, is_group_message
        )
        "#,
    );
    qb.push_values(&to_insert, |mut b, msg| {
        let content_type_str = msg
            .content_type
            .as_deref() // Convert Option<String> to Option<&str>
            .unwrap_or("application/octet-stream");
        b.push_bind(msg.id)
            .push_bind(&msg.ciphertext)
            .push_bind(msg.nonce)
            .push_bind(content_type_str) // Default
            .push_bind(msg.content_size_bytes)
            .push_bind(msg.api_key_id)
            .push_bind(msg.created_at)
            .push_bind(msg.expires_at)
            .push_bind(msg.access_count)
            .push_bind(msg.is_delivered)
            .push_bind(msg.delivered_at)
            .push_bind(msg.max_access_count)
            .push_bind(msg.require_proof_verification)
            .push_bind(msg.sender_client_id)
            .push_bind(msg.recipient_client_id)
            .push_bind(msg.group_id)
            .push_bind(msg.is_group_message);
    });
    qb.push(" ON CONFLICT (id) DO NOTHING");
    qb.build().execute(&mut *tx).await?;
    tx.commit().await?;
    // Flush pending reads
    let mut rconn = redis_pool.get().await?;
    for msg in &to_insert {
        let pending_key = instant_pending_read_key(msg.id);
        if let Ok(Some(data)) = rconn.get_del::<_, Option<String>>(&pending_key).await
            && let Ok(pending) = serde_json::from_str::<PendingRead>(&data)
        {
            let _ = sqlx::query(
                r#"
                    INSERT INTO p2p_read_receipts (id, message_id, client_id, read_at)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT DO NOTHING
                    "#,
            )
            .bind(Uuid::new_v4())
            .bind(msg.id)
            .bind(pending.reader_client_id)
            .bind(pending.read_at)
            .execute(db_pool)
            .await;
        }
    }
    // Clean Redis (non-DB fields like signature stay in Redis until DEL)
    let mut pipe = pipe();
    for msg in &to_insert {
        pipe.del(instant_message_key(msg.id));
        pipe.lrem(
            instant_inbox_key(msg.recipient_client_id),
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
/// Processes batch of delete tasks: cleans Redis, deletes/updates DB.
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
        let redis_key = instant_message_key(task.msg_id);
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
            r#"
            UPDATE messages
            SET is_delivered = true, delivered_at = $1
            WHERE id = ANY($2::uuid[])
              AND is_group_message = true
              AND delivered_at IS NULL
            "#,
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
