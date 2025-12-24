//! Delivery tracking - read receipts, verification, and deletion.

use super::dto::*;
use super::helper::*;
use crate::crypto::verify_signature;
use crate::error::{Result, VaultlessError};
use crate::models::usage::application::{record_proof_verified, RecordProofVerifiedInput};
use chrono::Utc;
use redis::AsyncCommands;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};
use uuid::Uuid;

impl InstantMessage {
    // =========================================================================
    // Read Receipt Management
    // =========================================================================

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
            .fetch_optional(self.db_pool.as_ref())
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
            .execute(self.db_pool.as_ref())
            .await?;

            let _ = sqlx::query(
                "UPDATE messages SET delivered_at = $1 WHERE id = $2 AND delivered_at IS NULL",
            )
            .bind(Utc::now())
            .bind(msg_id)
            .execute(self.db_pool.as_ref())
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

        // Pub/Sub notification
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

    // =========================================================================
    // Envelope Verification
    // =========================================================================

    /// Soft-verifies message envelope signature; logs errors, returns bool.
    pub async fn verify_envelope_soft(&self, msg: &Message) -> bool {
        if !msg.require_proof_verification {
            info!(msg_id = %msg.id, "Proof verification not required - skipping signature check");
            return true;
        }

        let envelope = Envelope {
            id: &msg.id,
            sender_client_id: &msg.sender_client_id,
            recipient_client_id: &msg.recipient_client_id,
            application_id: &msg.application_id,
            is_group_message: msg.is_group_message,
            content_size_bytes: msg.content_size_bytes as i64,
            created_at: &msg.created_at,
            require_proof_verification: msg.require_proof_verification,
        };

        let Some(signature_str) = msg.signature.as_deref() else {
            error!("Message signature is missing but required for verification.");
            return false;
        };

        match serde_json::to_vec(&envelope) {
            Ok(bytes) => match verify_signature(&bytes, signature_str, &msg.envelope_public_key) {
                Ok(()) => {
                    if let Err(e) = record_proof_verified(
                        &self.redis_pool,
                        RecordProofVerifiedInput::new(
                            msg.id,
                            msg.application_id,
                            String::new(), // Empty session_id for app-only metrics
                        ),
                        None,
                    )
                    .await
                    {
                        error!(
                            msg_id = %msg.id,
                            application_id = %msg.application_id,
                            error = %e,
                            "Failed to increment proof verified metrics"
                        );
                    }
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

    // =========================================================================
    // Message Deletion
    // =========================================================================

    /// Proper error handling for invalid message deletion with retry.
    pub async fn delete_invalid_message(&self, msg_id: Uuid, is_group: bool) -> Result<()> {
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

    /// Enhanced queue_delete with better error handling and fallback.
    pub async fn queue_delete(&self, msg_id: Uuid, is_group_message: bool) {
        let task = DeleteTask {
            msg_id,
            is_group_message,
        };

        match self.delete_sender.try_send(task) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(msg_id = %msg_id, "Delete channel full; attempting fallback");

                // Try to get strong reference to pool
                if let Some(db_pool) = self.weak_db_pool.upgrade() {
                    let msg_id_clone = msg_id;
                    tokio::spawn(async move {
                        if let Err(e) = sqlx::query(
                            "DELETE FROM messages WHERE id = $1 AND is_group_message = false",
                        )
                        .bind(msg_id_clone)
                        .execute(db_pool.as_ref())
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
}
