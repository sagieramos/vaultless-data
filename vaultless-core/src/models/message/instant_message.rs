use super::{dto::*, helper::*};
use crate::circuit_breaker::CircuitBreaker;
use crate::error::{Result, VaultlessError};
use crate::models::usage::{MetricsConfig, increment_message_sent_pool};
use chrono::{Duration as ChronoDuration, Utc};
use deadpool_redis::Pool as RedisPool;
use futures::future::join_all;
use redis::AsyncCommands;
use sqlx::{PgPool, query_as};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

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
    /// Fetches up to MAX_INBOX_FETCH undelivered messages for a recipient from Redis/DB.
    /// Verifies signatures, marks delivered, increments received metrics, and queues deletes.
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
}
