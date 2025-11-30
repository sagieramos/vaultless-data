use crate::circuit_breaker::CircuitBreaker;
use crate::models::usage::MetricsConfig;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;
use uuid::Uuid;

use std::sync::atomic::{AtomicUsize, Ordering};
/// Canonical envelope for signature verification.
#[derive(Serialize)]
pub struct Envelope<'a> {
    pub id: &'a Uuid,
    pub sender_client_id: &'a Uuid,
    pub recipient_client_id: &'a Uuid,
    pub api_key_id: &'a Uuid, // Aligned: non-optional per schema
    pub is_group_message: bool,
    pub content_size_bytes: i64,
    pub created_at: &'a DateTime<Utc>,
    pub require_proof_verification: bool,
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
    pub access_count: i64,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
    pub max_access_count: Option<i64>,
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
pub struct PendingRead {
    pub message_id: Uuid,
    pub reader_client_id: Uuid,
    pub read_at: DateTime<Utc>,
}
// =============================================================================
// Delete Task
// =============================================================================
/// Task for background message deletion.
#[derive(Debug, Clone)]
pub struct DeleteTask {
    pub msg_id: Uuid,
    pub is_group_message: bool,
}
// =============================================================================
// InstantMessage Core (with Weak<PgPool> for fallback)
// =============================================================================
/// Core struct for P2P instant messaging.
/// Manages Redis caching, DB persistence, metrics, and background tasks.
#[derive(Clone)]
pub struct InstantMessage {
    pub redis_pool: Arc<RedisPool>,
    pub db_pool: Arc<PgPool>,
    pub weak_db_pool: Weak<PgPool>,
    pub config: Arc<MetricsConfig>,
    pub sender: mpsc::Sender<Message>,
    pub delete_sender: mpsc::Sender<DeleteTask>,
    pub dlq_sender: mpsc::Sender<DlqEntry>,
    pub metrics: Arc<SystemMetrics>,
    // NEW: Circuit breakers
    pub redis_breaker: Arc<CircuitBreaker>,
    pub db_breaker: Arc<CircuitBreaker>,
}

/// Dead letter queue entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlqEntry {
    pub msg_id: Uuid,
    pub reason: DlqReason,
    pub timestamp: DateTime<Utc>,
    pub retry_count: u32,
    pub original_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DlqReason {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub flusher_channel_capacity: usize,
    pub flusher_channel_available: usize,
    pub deleter_channel_capacity: usize,
    pub deleter_channel_available: usize,
    pub dlq_channel_capacity: usize,
    pub dlq_channel_available: usize,
    pub failed_verifications: usize,
    pub failed_metrics_increments: usize,
    pub emergency_writes: usize,
    pub dlq_entries: usize,
    pub db_pool_dropped_deletes: usize,
    pub db_pool_available: bool,
    pub redis_circuit_state: String,
    pub db_circuit_state: String,
}
