//! Inbox management - rebuild and trim operations.

use super::dto::*;
use super::helper::*;
use crate::error::{Result, VaultlessError};
use redis::AsyncCommands;
use tracing::{error, info, warn};
use uuid::Uuid;

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
    // =========================================================================
    // Inbox Rebuild Operations
    // =========================================================================

    /// Rebuild inbox with pagination to avoid memory spikes.
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
            .fetch_all(self.db_pool.as_ref())
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

    /// Enhanced rebuild_inbox_safe using paginated approach with distributed lock.
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

    /// Rebuilds inbox queue from undelivered DB messages (non-paginated).
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
        .fetch_all(self.db_pool.as_ref())
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

        // Lua script: push all IDs and set TTL atomically
        let lua_script = r#"
            local ttl = tonumber(ARGV[#ARGV])
            for i = 1, #ARGV - 1 do
                redis.call("RPUSH", KEYS[1], ARGV[i])
            end
            redis.call("EXPIRE", KEYS[1], ttl)
            return #ARGV - 1
        "#;

        let mut args: Vec<String> = undelivered.iter().map(|id| id.to_string()).collect();
        args.push(CACHE_TTL_SECS.to_string());

        let _: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(1)
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

    // =========================================================================
    // Inbox Trim Operations
    // =========================================================================

    /// Efficient batch inbox trimming using Lua script with atomic RENAME.
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

        // Use RENAME for atomic replacement to avoid race condition
        let lua_script = r#"
            local queue_key = KEYS[1]
            local temp_key = KEYS[2]
            local ttl = tonumber(ARGV[#ARGV])
            local to_remove = {}

            -- Build hash set of IDs to remove (O(N))
            for i = 1, #ARGV - 1 do
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

            -- Atomic replacement using temp key + RENAME
            if #filtered > 0 then
                redis.call("DEL", temp_key)
                redis.call("RPUSH", temp_key, unpack(filtered))
                redis.call("RENAME", temp_key, queue_key)
                redis.call("EXPIRE", queue_key, ttl)
            else
                -- No items left, just delete the queue
                redis.call("DEL", queue_key)
            end

            return #items - #filtered
        "#;

        let mut args = id_strs;
        args.push(CACHE_TTL_SECS.to_string());

        let temp_key = format!("{}:trim_temp", queue_key);

        let removed: i32 = redis::cmd("EVAL")
            .arg(lua_script)
            .arg(2)
            .arg(&queue_key)
            .arg(&temp_key)
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

    /// Alternative for very large inboxes (>1000 items) using set membership.
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
            .arg(2)
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

    // =========================================================================
    // Atomic Delivery Counting
    // =========================================================================

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

    /// Retry wrapper for delivery counting with exponential backoff.
    pub async fn count_delivery_once_with_retry(&self, msg_id: Uuid) -> Result<bool> {
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 50;

        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            match self.redis_pool.get().await {
                Ok(mut conn) => match self.count_delivery_once_atomic(&mut conn, msg_id).await {
                    Ok(counted) => return Ok(counted),
                    Err(e) => {
                        last_error = Some(e);
                    }
                },
                Err(e) => {
                    last_error = Some(e.into());
                }
            }

            if attempt < MAX_RETRIES - 1 {
                use rand::Rng;
                let delay = BASE_DELAY_MS * (1 << attempt);
                let jitter = rand::rng().random_range(0..delay / 2);
                tokio::time::sleep(std::time::Duration::from_millis(delay + jitter)).await;
            }
        }

        Err(last_error
            .unwrap_or_else(|| VaultlessError::Internal("Delivery counting failed".into())))
    }
}
