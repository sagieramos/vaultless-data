use super::{dto::*, helper::*};
use redis::{AsyncCommands, pipe};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::interval};
use tracing::{error, info};
use uuid::Uuid;

impl InstantMessage {
    /// Background DLQ processor
    pub fn spawn_dlq_processor(&self, mut rx: mpsc::Receiver<DlqEntry>) {
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
    pub fn spawn_metrics_reporter(&self) {
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

    // -------------------------------------------------------------------------
    // Background flusher
    // -------------------------------------------------------------------------
    /// Spawns background task to flush message batches to DB.
    pub fn spawn_flusher(&self, mut rx: mpsc::Receiver<Message>) {
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
    pub fn spawn_deleter(&self, mut rx: mpsc::Receiver<DeleteTask>) {
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
    pub fn spawn_purger(&self) {
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
