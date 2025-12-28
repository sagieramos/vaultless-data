//! Message sending implementation using atomic Lua script.

use super::dto::*;
use super::helper::*;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::models::usage::client::{ClientMetricKey, MetricGranularity};

/// Default monthly message quota (100,000 messages per month)
const DEFAULT_MONTHLY_QUOTA: i64 = 100_000;

/// Default monthly client message quota (10,000 messages per month)
const DEFAULT_CLIENT_MONTHLY_QUOTA: i64 = 10_000;

/// Default per-minute rate limit for applications (1000 messages/min)
const DEFAULT_APP_RATE_LIMIT: i64 = 1000;

/// Default per-minute rate limit for clients (100 messages/min)
const DEFAULT_CLIENT_RATE_LIMIT: i64 = 100;

/// Lua script for atomic message sending (with optional proof verification)
const SEND_MESSAGE_LUA: &str = include_str!("../../scripts/send_message_v1.lua");

/// Message send result from Lua script
#[derive(Debug, Clone)]
pub struct SendMessageResult {
    pub status: String,       // "OK", "QUOTA_EXCEEDED", "DUPLICATE", "ERROR"
    pub counted: i64,         // 1 if counted, 0 if duplicate
    pub remaining_quota: i64, // Remaining monthly quota
    pub error_details: Option<String>,
}

impl InstantMessage {
    /// Sends a P2P instant message atomically via Lua script.
    ///
    /// This method performs all critical operations in a single Redis round-trip:
    /// 1. Signature verification (no Redis)
    /// 2. Idempotency check (prevents duplicate sending)
    /// 3. Monthly quota check and increment
    /// 4. Session metrics increment (including proof verified if applicable)
    /// 5. Message caching
    /// 6. Recipient inbox enqueue
    ///
    /// Returns immediately on duplicate detection without state changes.
    pub async fn send_instant_message(
        &self,
        sender_client_id: Uuid,
        recipient_client_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        content_size_bytes: i64,
        application_id: Uuid,
        session_id: String,
        signature: Option<String>,
        envelope_public_key: String,
        require_proof_verification: bool,
        encryption_algorithm: Option<String>,
        algorithm_version: Option<i16>,
    ) -> Result<Uuid> {
        // Create message ID first for idempotency
        let msg_id = Uuid::new_v4();
        let created_at = Utc::now();
        let expires_at = created_at + ChronoDuration::days(DEFAULT_MESSAGE_EXPIRY_DAYS);

        let msg = Message {
            id: msg_id,
            ciphertext,
            nonce,
            content_type: None,
            content_size_bytes,
            application_id,
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
            encryption_algorithm,
            algorithm_version,
            session_id: Some(session_id.clone()),
            signature,
            envelope_public_key,
            file_id: None,
        };

        // Verify signature (no Redis call - proof recorded in Lua script later)
        let proof_verified = verify_envelope_without_metrics(&msg);
        if !proof_verified {
            return Err(VaultlessError::SignatureVerificationFailed(format!(
                "Message {} failed signature verification",
                msg_id
            )));
        }

        // Execute atomic Lua script (includes proof verified and rate limit check)
        let result = self.execute_send_message(&msg, &session_id, proof_verified, 60, false).await?;

        // Handle result
        match result.status.as_str() {
            "OK" => {
                debug!(
                    msg_id = %msg_id,
                    remaining_quota = result.remaining_quota,
                    "Message sent successfully"
                );
            }
            "DUPLICATE" => {
                debug!(msg_id = %msg_id, "Message already sent (duplicate)");
                // Return the existing message ID - idempotent success
                return Ok(msg_id);
            }
            "QUOTA_EXCEEDED" => {
                error!(
                    msg_id = %msg_id,
                    application_id = %application_id,
                    "Monthly quota exceeded"
                );
                return Err(VaultlessError::QuotaExceeded(
                    result.error_details.unwrap_or_else(|| "Monthly quota limit reached".into()),
                ));
            }
            "RATE_LIMIT_EXCEEDED" => {
                error!(
                    msg_id = %msg_id,
                    "Per-minute rate limit exceeded"
                );
                return Err(VaultlessError::RateLimitExceeded(
                    result.error_details.unwrap_or_else(|| "Per-minute rate limit exceeded".into()),
                ));
            }
            "ERROR" => {
                error!(
                    msg_id = %msg_id,
                    error = ?result.error_details,
                    "Redis error during message send"
                );
                return Err(VaultlessError::Internal(
                    result.error_details.unwrap_or_else(|| "Redis operation failed".into()),
                ));
            }
            _ => {
                error!(msg_id = %msg_id, status = %result.status, "Unknown send status");
                return Err(VaultlessError::Internal(format!(
                    "Unknown send status: {}",
                    result.status
                )));
            }
        };

        // Non-critical path: queue for database flush
        // If this fails, message is still in Redis cache
        self.queue_for_persistence(&msg).await;

        info!(
            msg_id = %msg_id,
            sender = %sender_client_id,
            recipient = %recipient_client_id,
            size_bytes = content_size_bytes,
            "Message sent successfully"
        );

        Ok(msg_id)
    }

    /// Executes the atomic send message Lua script
    async fn execute_send_message(
        &self,
        msg: &Message,
        session_id: &str,
        proof_verified: bool,
        _rate_limit_per_minute: i64, // Deprecated, now uses separate app/client rate limits
        persist_to_db: bool,
    ) -> Result<SendMessageResult> {
        let mut conn = self.redis_pool.get().await?;
        let now = chrono::Utc::now();

        // Generate Redis keys
        let app_quota_key = cache_key!("quota", "app", msg.application_id, "monthly");
        let minute_window = crate::models::usage::application::get_minute_window(&now);
        let minute_key = minute_window.format("%Y_%m_%d_%H_%M").to_string();

        // Application and client rate limit keys (per-minute)
        let app_rate_limit_key = cache_key!("metric", "app", msg.application_id, "minute", minute_key);
        let client_rate_limit_key = cache_key!("metric", "client", msg.sender_client_id, "minute", minute_key);

        let session_sent_key = cache_key!("metric", "session", session_id, "sent");
        let session_bytes_key = cache_key!("metric", "session", session_id, "bytes_sent");
        let session_proved_key = cache_key!("metric", "session", session_id, "proved");
        let idempotency_key = cache_key!("counted", "msg", msg.id);
        let message_stream_key = cache_key!("stream", "instant_message", "pending");

        // Client quota and metric keys
        let client_quota_key = cache_key!("quota", "client", msg.sender_client_id, "monthly");
        let client_metric_key = ClientMetricKey::new(
            msg.application_id, msg.sender_client_id, now,
            MetricGranularity::Hour
        )?.as_str().to_string();
        let client_active_keys_set = cache_key!("metric", "client", "active_keys");

        // Serialize message
        let message_json = serde_json::to_string(msg)
            .map_err(|e| VaultlessError::Internal(format!("Failed to serialize message: {}", e)))?;

        // Execute Lua script
        let result: Vec<String> = tokio::time::timeout(
            Duration::from_secs(5),
            redis::Script::new(SEND_MESSAGE_LUA)
                .key(&app_quota_key)
                .key(&app_rate_limit_key)
                .key(&client_rate_limit_key)
                .key(&session_sent_key)
                .key(&session_bytes_key)
                .key(&session_proved_key)
                .key(&idempotency_key)
                .key(&message_stream_key)
                .key(&client_quota_key)
                .key(&client_metric_key)
                .key(&client_active_keys_set)
                .arg(DEFAULT_MONTHLY_QUOTA)                  // ARGV[1]: app monthly quota
                .arg(DEFAULT_APP_RATE_LIMIT)                 // ARGV[2]: app rate limit per minute
                .arg(DEFAULT_CLIENT_MONTHLY_QUOTA)           // ARGV[3]: client monthly quota
                .arg(DEFAULT_CLIENT_RATE_LIMIT)              // ARGV[4]: client rate limit per minute
                .arg(7 * 24 * 60 * 60)                       // ARGV[5]: session TTL (7 days)
                .arg(3600)                                    // ARGV[6]: idempotency TTL (1 hour)
                .arg(100000)                                  // ARGV[7]: stream max length
                .arg(msg.id.to_string())                      // ARGV[8]: message_id
                .arg(&message_json)                           // ARGV[9]: message JSON
                .arg(msg.content_size_bytes)                  // ARGV[10]: size_bytes
                .arg(session_id)                              // ARGV[11]: session_id
                .arg(msg.recipient_client_id.to_string())     // ARGV[12]: recipient
                .arg(if proof_verified { 1 } else { 0 })      // ARGV[13]: proof_verified
                .arg(if persist_to_db { 1 } else { 0 })       // ARGV[14]: persist_to_db
                .arg(7 * 24 * 60 * 60)                        // ARGV[15]: client metric TTL (7 days)
                .arg(msg.sender_client_id.to_string())        // ARGV[16]: sender client ID
                .invoke_async(&mut conn),
        )
        .await
        .map_err(|_| VaultlessError::Timeout("send_message Lua script timed out".into()))?
        .map_err(|e| VaultlessError::Internal(format!("Lua script error: {}", e)))?;

        // Parse result
        let status = result.get(0).cloned().unwrap_or_else(|| "ERROR".to_string());
        let counted = result
            .get(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let remaining_quota = result
            .get(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let error_details = result.get(3).cloned();

        Ok(SendMessageResult {
            status,
            counted,
            remaining_quota,
            error_details,
        })
    }

    /// Queues message for async persistence to PostgreSQL
    /// This is non-critical path - message is already in Redis cache
    async fn queue_for_persistence(&self, msg: &Message) {
        if let Err(mpsc::error::TrySendError::Full(msg_clone)) = self.sender.try_send(msg.clone()) {
            warn!(
                msg_id = %msg.id,
                "Flusher channel full; attempting emergency write"
            );
            self.emergency_write(&msg_clone).await;
        } else {
            // Message queued successfully, nothing to do
        }
    }

    /// Emergency write to database when channel is full
    async fn emergency_write(&self, msg: &Message) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let db_pool = Arc::clone(&self.db_pool);
        let msg_clone = msg.clone();

        tokio::spawn(async move {
            let result = emergency_write_message(&db_pool, &msg_clone).await;
            let _ = tx.send(result);
        });

        match tokio::time::timeout(Duration::from_secs(5), rx).await {
            Ok(Ok(Ok(()))) => {
                info!(msg_id = %msg.id, "Emergency write succeeded");
            }
            Ok(Ok(Err(e))) => {
                error!(
                    msg_id = %msg.id,
                    error = %e,
                    "Emergency write failed - message may be lost"
                );
            }
            Ok(Err(_)) => {
                error!(msg_id = %msg.id, "Emergency write channel dropped");
            }
            Err(_) => {
                error!(msg_id = %msg.id, "Emergency write timeout");
            }
        }
    }
}

/// Emergency write to database for messages that can't be queued
async fn emergency_write_message(
    db_pool: &sqlx::PgPool,
    msg: &Message,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO messages (
            id, ciphertext, nonce, content_type, content_size_bytes, application_id,
            created_at, expires_at, accessed_at, access_count, is_delivered,
            delivered_at, max_access_count, require_proof_verification,
            sender_client_id, recipient_client_id, group_id, is_group_message,
            signature, envelope_public_key, file_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)
        "#,
    )
    .bind(msg.id)
    .bind(&msg.ciphertext)
    .bind(msg.nonce)
    .bind(msg.content_type.clone())
    .bind(msg.content_size_bytes)
    .bind(msg.application_id)
    .bind(msg.created_at)
    .bind(msg.expires_at)
    .bind(msg.accessed_at)
    .bind(msg.access_count)
    .bind(msg.is_delivered)
    .bind(msg.delivered_at)
    .bind(msg.max_access_count)
    .bind(msg.require_proof_verification)
    .bind(msg.sender_client_id)
    .bind(msg.recipient_client_id)
    .bind(msg.group_id)
    .bind(msg.is_group_message)
    .bind(msg.signature.clone())
    .bind(&msg.envelope_public_key)
    .bind(&msg.file_id)
    .execute(db_pool)
    .await
    .map_err(|e| VaultlessError::Internal(format!("Emergency write failed: {}", e)))?;

    Ok(())
}
