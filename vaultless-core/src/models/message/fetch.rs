//! Message fetching and inbox operations.

use super::dto::*;
use super::helper::*;
use crate::error::{Result, VaultlessError};
use chrono::Utc;
use futures::future::join_all;
use redis::AsyncCommands;
use sqlx::query_as;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};
use uuid::Uuid;

impl InstantMessage {
    /// Returns inbox status for a recipient without fetching full messages.
    /// Used to inform clients about pending messages on WebSocket connect.
    pub async fn get_inbox_status(&self, recipient_client_id: Uuid) -> Result<InboxStatus> {
        let mut conn = self.redis_pool.get().await?;
        let queue_key = instant_inbox_key(recipient_client_id);

        // Get inbox length from Redis
        let inbox_len: isize = conn
            .llen(&queue_key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if inbox_len == 0 {
            // Check if we need to rebuild from DB
            let db_count: Option<(i64,)> = sqlx::query_as(
                "SELECT COUNT(*) FROM messages WHERE recipient_client_id = $1 AND is_delivered = false",
            )
            .bind(recipient_client_id)
            .fetch_optional(self.db_pool.as_ref())
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            let unread_count = db_count.map(|(c,)| c as usize).unwrap_or(0);

            if unread_count == 0 {
                return Ok(InboxStatus {
                    unread_count: 0,
                    oldest_unread_at: None,
                    newest_unread_at: None,
                    total_size_bytes: 0,
                });
            }

            // Get timestamp range and total size from DB
            let stats: Option<(
                Option<chrono::DateTime<Utc>>,
                Option<chrono::DateTime<Utc>>,
                Option<i64>,
            )> = sqlx::query_as(
                "SELECT MIN(created_at), MAX(created_at), SUM(content_size_bytes) FROM messages WHERE recipient_client_id = $1 AND is_delivered = false",
            )
            .bind(recipient_client_id)
            .fetch_optional(self.db_pool.as_ref())
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            let (oldest, newest, total_size) = stats.unwrap_or((None, None, None));

            return Ok(InboxStatus {
                unread_count,
                oldest_unread_at: oldest,
                newest_unread_at: newest,
                total_size_bytes: total_size.unwrap_or(0),
            });
        }

        // Get message IDs from Redis inbox
        let msg_id_strs: Vec<String> = conn
            .lrange(&queue_key, 0, -1)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let msg_ids: Vec<Uuid> = msg_id_strs
            .iter()
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();

        if msg_ids.is_empty() {
            return Ok(InboxStatus {
                unread_count: 0,
                oldest_unread_at: None,
                newest_unread_at: None,
                total_size_bytes: 0,
            });
        }

        // Bulk fetch message metadata from Redis
        let redis_keys: Vec<String> = msg_ids.iter().map(|id| instant_message_key(*id)).collect();
        let results: Vec<Option<String>> = conn
            .mget(&redis_keys)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let mut oldest_at: Option<chrono::DateTime<Utc>> = None;
        let mut newest_at: Option<chrono::DateTime<Utc>> = None;
        let mut total_size: i64 = 0;
        let mut valid_count: usize = 0;

        for data_opt in results.into_iter().flatten() {
            if let Ok(msg) = serde_json::from_str::<Message>(&data_opt) {
                valid_count += 1;
                total_size += msg.content_size_bytes;

                match &oldest_at {
                    None => oldest_at = Some(msg.created_at),
                    Some(existing) if msg.created_at < *existing => oldest_at = Some(msg.created_at),
                    _ => {}
                }

                match &newest_at {
                    None => newest_at = Some(msg.created_at),
                    Some(existing) if msg.created_at > *existing => newest_at = Some(msg.created_at),
                    _ => {}
                }
            }
        }

        // If some messages weren't in Redis, fall back to DB for accurate stats
        if valid_count < msg_ids.len() {
            let missing_ids: Vec<Uuid> = msg_ids.iter().skip(valid_count).cloned().collect();
            if !missing_ids.is_empty() {
                let db_stats: Option<(
                    Option<chrono::DateTime<Utc>>,
                    Option<chrono::DateTime<Utc>>,
                    Option<i64>,
                    i64,
                )> = sqlx::query_as(
                    "SELECT MIN(created_at), MAX(created_at), SUM(content_size_bytes), COUNT(*) FROM messages WHERE id = ANY($1::uuid[]) AND is_delivered = false",
                )
                .bind(&missing_ids)
                .fetch_optional(self.db_pool.as_ref())
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;

                if let Some((db_oldest, db_newest, db_size, db_count)) = db_stats {
                    valid_count += db_count as usize;
                    total_size += db_size.unwrap_or(0);

                    if let Some(db_old) = db_oldest {
                        oldest_at = Some(match oldest_at {
                            None => db_old,
                            Some(existing) if db_old < existing => db_old,
                            Some(existing) => existing,
                        });
                    }

                    if let Some(db_new) = db_newest {
                        newest_at = Some(match newest_at {
                            None => db_new,
                            Some(existing) if db_new > existing => db_new,
                            Some(existing) => existing,
                        });
                    }
                }
            }
        }

        Ok(InboxStatus {
            unread_count: valid_count,
            oldest_unread_at: oldest_at,
            newest_unread_at: newest_at,
            total_size_bytes: total_size,
        })
    }

    /// Returns inbox messages grouped by sender public key, with only the last
    /// message per sender. This is a read-only operation with NO side effects.
    pub async fn peek_inbox_grouped(&self, recipient_client_id: Uuid) -> Result<GroupedInbox> {
        use std::collections::HashMap;

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
                    "Failed to rebuild inbox for peek"
                );
            }
        }

        // Fetch message IDs from Redis inbox
        let msg_id_strs: Vec<String> = conn
            .lrange(&queue_key, 0, (MAX_INBOX_FETCH - 1) as isize)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if msg_id_strs.is_empty() {
            return Ok(GroupedInbox {
                entries: vec![],
                sender_count: 0,
                total_messages: 0,
            });
        }

        let msg_ids: Vec<Uuid> = msg_id_strs
            .iter()
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();

        if msg_ids.is_empty() {
            return Ok(GroupedInbox {
                entries: vec![],
                sender_count: 0,
                total_messages: 0,
            });
        }

        // Bulk fetch from Redis
        let redis_keys: Vec<String> = msg_ids.iter().map(|id| instant_message_key(*id)).collect();
        let results: Vec<Option<String>> = conn
            .mget(&redis_keys)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // Collect messages from Redis
        let mut messages: Vec<Message> = Vec::new();
        let mut fallback_ids: Vec<Uuid> = Vec::new();

        for (data_opt, msg_id) in results.into_iter().zip(msg_ids.iter()) {
            match data_opt {
                Some(data) => {
                    if let Ok(msg) = serde_json::from_str::<Message>(&data) {
                        messages.push(msg);
                    } else {
                        fallback_ids.push(*msg_id);
                    }
                }
                None => {
                    fallback_ids.push(*msg_id);
                }
            }
        }

        // SQL fallback for cache misses (read-only, no state changes)
        if !fallback_ids.is_empty() {
            let sql_msgs = fetch_sql_fallback(&self.db_pool, &fallback_ids, recipient_client_id)
                .await
                .unwrap_or_default();
            messages.extend(sql_msgs);
        }

        let total_messages = messages.len();

        // Group by sender public key
        let mut grouped: HashMap<String, (usize, usize)> = HashMap::new();

        for (idx, msg) in messages.iter().enumerate() {
            let sender_pubkey = &msg.envelope_public_key;

            grouped
                .entry(sender_pubkey.clone())
                .and_modify(|(last_idx, count)| {
                    *count += 1;
                    if msg.created_at > messages[*last_idx].created_at {
                        *last_idx = idx;
                    }
                })
                .or_insert((idx, 1));
        }

        // Build entries
        let mut index_entries: Vec<(usize, String, usize)> = grouped
            .into_iter()
            .map(|(pubkey, (idx, count))| (idx, pubkey, count))
            .collect();

        // Sort by index descending for safe swap_remove
        index_entries.sort_by(|a, b| b.0.cmp(&a.0));

        let mut entries: Vec<InboxEntry> = index_entries
            .into_iter()
            .map(|(idx, sender_pubkey, message_count)| {
                let last_message = messages.swap_remove(idx);
                InboxEntry {
                    sender_pubkey,
                    last_message,
                    message_count,
                }
            })
            .collect();

        // Sort by last message created_at descending
        entries.sort_by(|a, b| b.last_message.created_at.cmp(&a.last_message.created_at));

        let sender_count = entries.len();

        info!(
            recipient = %recipient_client_id,
            sender_count,
            total_messages,
            "Peeked grouped inbox"
        );

        Ok(GroupedInbox {
            entries,
            sender_count,
            total_messages,
        })
    }

    /// Returns paginated messages from a specific sender (identified by public key).
    /// This is a read-only operation with NO side effects.
    pub async fn fetch_messages_by_sender(
        &self,
        recipient_client_id: Uuid,
        sender_pubkey: &str,
        offset: usize,
        limit: usize,
    ) -> Result<SenderMessages> {
        use chrono::{DateTime, Utc};

        let limit = limit.min(100);
        let fetch_limit = offset + limit + 1;

        let mut conn = self.redis_pool.get().await?;
        let queue_key = instant_inbox_key(recipient_client_id);

        // Step 1: Fetch from Redis
        let msg_id_strs: Vec<String> = conn
            .lrange(&queue_key, 0, (MAX_INBOX_FETCH - 1) as isize)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let msg_ids: Vec<Uuid> = msg_id_strs
            .iter()
            .filter_map(|s| Uuid::parse_str(s).ok())
            .collect();

        let mut redis_messages: Vec<Message> = Vec::new();
        let mut oldest_redis_timestamp: Option<DateTime<Utc>> = None;
        let mut sender_client_id: Option<Uuid> = None;

        if !msg_ids.is_empty() {
            let redis_keys: Vec<String> =
                msg_ids.iter().map(|id| instant_message_key(*id)).collect();
            let results: Vec<Option<String>> = conn
                .mget(&redis_keys)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            for data_opt in results.into_iter().flatten() {
                if let Ok(msg) = serde_json::from_str::<Message>(&data_opt) {
                    if msg.envelope_public_key == sender_pubkey {
                        if sender_client_id.is_none() {
                            sender_client_id = Some(msg.sender_client_id);
                        }

                        match oldest_redis_timestamp {
                            None => oldest_redis_timestamp = Some(msg.created_at),
                            Some(ts) if msg.created_at < ts => {
                                oldest_redis_timestamp = Some(msg.created_at)
                            }
                            _ => {}
                        }

                        redis_messages.push(msg);
                    }
                }
            }
        }

        // Step 2: Resolve sender_client_id if not found in Redis
        if sender_client_id.is_none() {
            let client: Option<(Uuid,)> =
                sqlx::query_as("SELECT id FROM clients WHERE public_key = $1 LIMIT 1")
                    .bind(sender_pubkey)
                    .fetch_optional(self.db_pool.as_ref())
                    .await
                    .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            sender_client_id = client.map(|(id,)| id);
        }

        // Step 3: Fetch from DB
        let mut db_messages: Vec<Message> = Vec::new();

        if let Some(sender_id) = sender_client_id {
            let db_msgs: Vec<Message> = if let Some(oldest_ts) = oldest_redis_timestamp {
                sqlx::query_as(
                    r#"
                    SELECT id, ciphertext, nonce, content_type, content_size_bytes,
                           application_id, created_at, expires_at, accessed_at, access_count,
                           is_delivered, delivered_at, max_access_count,
                           require_proof_verification, sender_client_id, recipient_client_id,
                           group_id, is_group_message,
                           '' as signature, '' as envelope_public_key, NULL::uuid as file_id
                    FROM messages
                    WHERE recipient_client_id = $1
                      AND sender_client_id = $2
                      AND created_at < $3
                    ORDER BY created_at DESC
                    LIMIT $4
                    "#,
                )
                .bind(recipient_client_id)
                .bind(sender_id)
                .bind(oldest_ts)
                .bind(fetch_limit as i64)
                .fetch_all(self.db_pool.as_ref())
                .await
                .unwrap_or_default()
            } else {
                sqlx::query_as(
                    r#"
                    SELECT id, ciphertext, nonce, content_type, content_size_bytes,
                           application_id, created_at, expires_at, accessed_at, access_count,
                           is_delivered, delivered_at, max_access_count,
                           require_proof_verification, sender_client_id, recipient_client_id,
                           group_id, is_group_message,
                           '' as signature, '' as envelope_public_key, NULL::uuid as file_id
                    FROM messages
                    WHERE recipient_client_id = $1
                      AND sender_client_id = $2
                    ORDER BY created_at DESC
                    LIMIT $3
                    "#,
                )
                .bind(recipient_client_id)
                .bind(sender_id)
                .bind(fetch_limit as i64)
                .fetch_all(self.db_pool.as_ref())
                .await
                .unwrap_or_default()
            };

            for mut msg in db_msgs {
                msg.envelope_public_key = sender_pubkey.to_string();
                db_messages.push(msg);
            }
        }

        // Step 4: Merge and sort
        let mut all_messages: Vec<Message> = redis_messages;
        all_messages.extend(db_messages);
        all_messages.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        all_messages.dedup_by(|a, b| a.id == b.id);

        let total = all_messages.len();

        // Step 5: Apply pagination
        let paginated: Vec<Message> = all_messages.into_iter().skip(offset).take(limit).collect();

        let count = paginated.len();
        let has_more = offset + count < total;

        info!(
            recipient = %recipient_client_id,
            sender_pubkey = %sender_pubkey,
            total,
            offset,
            returned = count,
            has_more,
            "Fetched messages by sender (Redis + DB)"
        );

        Ok(SenderMessages {
            sender_pubkey: sender_pubkey.to_string(),
            messages: paginated,
            total,
            offset,
            has_more,
        })
    }

    /// Fetches read receipts for a message from DB.
    pub async fn fetch_read_receipts(&self, msg_id: Uuid) -> Result<Vec<ReadReceipt>> {
        let receipts = query_as::<_, ReadReceipt>(
            "SELECT id, message_id, client_id, read_at FROM p2p_read_receipts WHERE message_id = $1",
        )
        .bind(msg_id)
        .fetch_all(self.db_pool.as_ref())
        .await?;
        Ok(receipts)
    }

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

        for (data_opt, msg_id) in results.into_iter().zip(msg_ids.into_iter()) {
            match data_opt {
                Some(data) => {
                    hit_zips.push((data, msg_id));
                }
                None => {
                    fallback_ids.push(msg_id);
                }
            }
        }

        let self_clone = self.clone();
        let recipient_clone = recipient_client_id;

        // Limit parallelism to avoid thundering herd
        let semaphore = Arc::new(Semaphore::new(10));

        // Process Redis hits with controlled parallelism
        let hit_futures: Vec<_> = hit_zips
            .into_iter()
            .map(|(data, msg_id)| {
                let self_clone = self_clone.clone();
                let sem = Arc::clone(&semaphore);

                async move {
                    let _permit = sem.acquire().await.ok()?;

                    let mut msg = match serde_json::from_str::<Message>(&data) {
                        Ok(m) => m,
                        Err(e) => {
                            error!(msg_id = %msg_id, error = %e, "Deserialization failed");
                            let _ = self_clone.delete_invalid_message(msg_id, false).await;
                            return None;
                        }
                    };

                    // Step 1: Verify signature FIRST
                    if !self_clone.verify_envelope_soft(&msg).await {
                        error!(msg_id = %msg_id, "Signature verification failed");
                        let _ = self_clone
                            .delete_invalid_message(msg_id, msg.is_group_message)
                            .await;
                        return None;
                    }

                    // Step 2: Atomically count delivery
                    let counted = match self_clone.count_delivery_once_with_retry(msg_id).await {
                        Ok(c) => c,
                        Err(e) => {
                            error!(
                                msg_id = %msg_id,
                                error = %e,
                                "Failed to count delivery - aborting message fetch"
                            );
                            return None;
                        }
                    };

                    // Step 3: Increment metrics
                    if counted {
                        match self_clone
                            .increment_received_metrics_with_retry(
                                msg.application_id,
                                msg.content_size_bytes as i64,
                            )
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    msg_id = %msg_id,
                                    application_id = %msg.application_id,
                                    "Delivery counted successfully"
                                );
                            }
                            Err(e) => {
                                error!(
                                    msg_id = %msg_id,
                                    error = %e,
                                    "Metrics increment failed - delivery NOT marked"
                                );
                                return None;
                            }
                        }
                    }

                    // Step 4: Mark as delivered
                    msg.is_delivered = true;
                    msg.delivered_at = Some(Utc::now());

                    // Step 5: Queue for deletion
                    self_clone.queue_delete(msg_id, msg.is_group_message).await;

                    Some(msg)
                }
            })
            .collect();

        let hit_results: Vec<Option<Message>> = join_all(hit_futures).await;
        let mut messages: Vec<Message> = hit_results.into_iter().flatten().collect();

        let from_redis = messages.len();

        // Only trim successfully processed message IDs
        let successful_redis_ids: Vec<Uuid> = messages.iter().map(|m| m.id).collect();
        if !successful_redis_ids.is_empty() {
            self_clone
                .trim_inbox_batch(&successful_redis_ids, recipient_client_id)
                .await?;
        }

        // SQL fallback with same atomic guarantees
        let mut from_sql = 0;
        if !fallback_ids.is_empty() {
            let sql_msgs = match fetch_sql_fallback(
                &self_clone.db_pool,
                &fallback_ids,
                recipient_client_id,
            )
            .await
            {
                Ok(msgs) => msgs,
                Err(e) => {
                    error!(
                        recipient = %recipient_client_id,
                        fallback_count = fallback_ids.len(),
                        error = %e,
                        "SQL fallback query failed - some messages may not be delivered"
                    );
                    vec![]
                }
            };

            let sql_futures: Vec<_> = sql_msgs
                .into_iter()
                .map(|mut msg| {
                    let self_clone = self_clone.clone();
                    let sem = Arc::clone(&semaphore);

                    async move {
                        let _permit = sem.acquire().await.ok()?;

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

                        let counted = match self_clone.count_delivery_once_with_retry(msg.id).await
                        {
                            Ok(c) => c,
                            Err(e) => {
                                error!(
                                    msg_id = %msg.id,
                                    error = %e,
                                    "SQL fallback: Failed to count delivery - aborting"
                                );
                                return None;
                            }
                        };

                        if counted {
                            if let Err(e) = self_clone
                                .increment_received_metrics_with_retry(
                                    msg.application_id,
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
            from_sql = sql_msgs.len();

            // Only trim successfully processed SQL message IDs
            let successful_sql_ids: Vec<Uuid> = sql_msgs.iter().map(|m| m.id).collect();
            if !successful_sql_ids.is_empty() {
                self_clone
                    .trim_inbox_batch(&successful_sql_ids, recipient_client_id)
                    .await?;
            }

            messages.extend(sql_msgs);
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
