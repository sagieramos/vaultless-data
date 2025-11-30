use super::{dto::*, helper::*};
use crate::crypto::verify_signature;
use crate::error::{Result, VaultlessError};
use crate::models::usage::increment_message_received_pool;
use crate::models::usage::increment_proof_verified_pool;
use chrono::Utc;
use redis::AsyncCommands;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

const CACHE_TTL_SECS: u64 = 600;
const MAX_QUEUE_LEN: isize = 10_000;


// Lua script for atomic delivery counting
const ATOMIC_DELIVERY_COUNT_SCRIPT: &str = r#"
local counted_key = KEYS[1]
-- Atomically check and set the counted flag
if redis.call('SET', counted_key, '1', 'NX', 'EX', 86400) then
  return 1
end
return 0
"#;
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
    pub async fn rebuild_inbox_safe(
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
    pub async fn verify_envelope_soft(&self, msg: &Message) -> bool {
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
            error!("Message signature is missing but required for verification.");
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
                        error!(
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
    pub async fn count_delivery_once_atomic(
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
    pub async fn rebuild_inbox(
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

    // FIX #3: Proper error handling for invalid message deletion
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

    /// Efficient batch inbox trimming using Lua script
    pub async fn trim_inbox_batch(
        &self,
        msg_ids: &[Uuid],
        recipient_client_id: Uuid,
    ) -> Result<()> {
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
    pub async fn trim_inbox_batch_large(
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

    /// Enhanced queue_delete with better error handling
    pub async fn queue_delete(&self, msg_id: Uuid, is_group_message: bool) {
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

    // Helper: Retry wrapper for delivery counting
    pub async fn count_delivery_once_with_retry(&self, msg_id: Uuid) -> Result<bool> {
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
    pub async fn increment_received_metrics_with_retry(
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
}
