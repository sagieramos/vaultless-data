//! Message sending implementation.

use super::dto::*;
use super::helper::*;
use crate::error::{Result, VaultlessError};
use crate::models::usage::increment_message_sent_pool;
use chrono::{Duration as ChronoDuration, Utc};
use redis::AsyncCommands;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

impl InstantMessage {
    /// Sends a P2P instant message, caches in Redis, queues for DB flush, and increments sent metrics.
    pub async fn send_instant_message(
        &self,
        sender_client_id: Uuid,
        recipient_client_id: Uuid,
        ciphertext: String,
        nonce: Uuid,
        content_size_bytes: i64,
        application_id: Uuid,
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
            signature,
            envelope_public_key,
            file_id: None,
        };

        // Verify signature BEFORE any state changes
        if !self.verify_envelope_soft(&msg).await {
            return Err(VaultlessError::SignatureVerificationFailed(format!(
                "Message {} failed signature verification",
                msg_id
            )));
        }

        // Increment sent metrics with atomic check
        let mut conn = self.redis_pool.get().await?;
        let counted_key = instant_sent_counted_key(msg_id);

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
                    msg.application_id,
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
                    application_id = %msg.application_id,
                    error = ?last_error,
                    "CRITICAL: Metrics increment failed after retries - billing affected"
                );
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

        // Handle backpressure with better error propagation
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
}
