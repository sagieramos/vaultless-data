//! Background flusher for periodic Redis to Postgres session counter persistence.
//!
//! Handles batched upserts for session message counters (sent, received, proved).
//! Uses SCAN to find all session metric keys since they're keyed by session_id.

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::models::usage::{session_metric_key, EngineConfig, RedisPoolType};
use redis::AsyncCommands;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{Duration, interval};
use tracing::{error, info};

/// Metrics for monitoring the session flusher performance
#[derive(Debug, Default)]
pub struct SessionFlusherMetrics {
    pub sessions_flushed: AtomicU64,
    pub errors: AtomicU64,
    pub total_flush_duration_ms: AtomicU64,
}

impl SessionFlusherMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }

    pub fn average_flush_duration_ms(&self) -> f64 {
        let duration = self.total_flush_duration_ms.load(Ordering::SeqCst);
        let flushed = self.sessions_flushed.load(Ordering::SeqCst);
        if flushed > 0 {
            duration as f64 / flushed as f64
        } else {
            0.0
        }
    }
}

/// Start the background session flusher task
///
/// Returns a join handle and a shutdown notifier. Call `shutdown.notify_one()`
/// to trigger graceful shutdown with a final flush.
pub fn start_session_flusher(
    redis_pool: Arc<RedisPoolType>,
    pg_pool: Arc<PgPool>,
    config: Arc<EngineConfig>,
    metrics: Option<Arc<SessionFlusherMetrics>>,
) -> (tokio::task::JoinHandle<()>, Arc<Notify>) {
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60)); // 60 second flush interval

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = flush_session_counters(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        false,
                    ).await {
                        error!(?e, "Session counters flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.record_error();
                        }
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received. Performing final session counters flush...");
                    if let Err(e) = flush_session_counters(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        true,
                    ).await {
                        error!(?e, "Final session flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.record_error();
                        }
                    } else {
                        info!("Final session flush completed successfully");
                    }
                    break;
                }
            }
        }
    });

    (handle, shutdown_clone)
}

/// Main flush cycle: scan Redis, collect counters, flush to Postgres
async fn flush_session_counters(
    redis_pool: Arc<RedisPoolType>,
    pg: Arc<PgPool>,
    config: &EngineConfig,
    metrics: Option<&SessionFlusherMetrics>,
    flush_all: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    // Track processed session IDs to avoid duplicates in this batch
    let mut processed_sessions: HashSet<String> = HashSet::new();
    // Batch format: (session_id, sent, received, proved, bytes_sent, bytes_received)
    let mut batch: Vec<(String, i64, i64, i64, i64, i64)> = Vec::new();

    // SCAN for all session metric keys
    let mut cursor: u64 = 0;
    let pattern = format!("{}:*", cache_key!("session", "metric"));

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = tokio::time::timeout(
            Duration::from_secs(config.operation_timeout_secs),
            redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(1000)
                .query_async(&mut *conn),
        )
        .await
        .map_err(|_| VaultlessError::Timeout("Redis SCAN timed out".into()))?
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // Process keys in this batch
        for key in keys {
            // Parse the session_id from the key
            // Key format: session:metric:{session_id}:{counter_type}
            let session_id = extract_session_id_from_key(&key);

            if session_id.is_empty() {
                continue;
            }

            // Skip if we already processed this session in this batch
            if !processed_sessions.insert(session_id.clone()) {
                continue;
            }

            // Get counters for this session
            let counters = get_session_counters(&mut conn, &session_id).await?;

            if let Some((sent, received, proved, bytes_sent, bytes_received)) = counters {
                if sent > 0 || received > 0 || proved > 0 || bytes_sent > 0 || bytes_received > 0 {
                    batch.push((session_id, sent, received, proved, bytes_sent, bytes_received));
                }
            }

            // Flush batch if it gets too large
            if batch.len() >= config.max_batch_size {
                flush_batch_to_pg(&pg, &batch, metrics).await?;
                batch.clear();
            }
        }

        cursor = next_cursor;
        if cursor == 0 || flush_all {
            break;
        }
    }

    // Flush any remaining items
    if !batch.is_empty() {
        flush_batch_to_pg(&pg, &batch, metrics).await?;
    }

    let duration = start.elapsed();
    let sessions_flushed = batch.len() as u64;

    if sessions_flushed > 0 {
        info!(
            sessions_flushed = sessions_flushed,
            duration_ms = duration.as_millis(),
            "Completed session counters flush"
        );
    }

    if let Some(metrics) = metrics {
        metrics
            .sessions_flushed
            .fetch_add(sessions_flushed, Ordering::SeqCst);
        metrics
            .total_flush_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
    }

    Ok(())
}

/// Extract session_id from a Redis key
/// Key format: session:metric:{session_id}:{counter_type}
fn extract_session_id_from_key(key: &str) -> String {
    let prefix = cache_key!("session", "metric");
    if !key.starts_with(&prefix) {
        return String::new();
    }

    // Remove prefix
    let after_prefix = &key[prefix.len()..];

    // Split by : and get the session_id (first part after prefix)
    let parts: Vec<&str> = after_prefix.trim_matches(':').split(':').collect();
    if parts.is_empty() {
        return String::new();
    }

    parts[0].to_string()
}

/// Get session counters from Redis (including bytes)
async fn get_session_counters<C>(
    conn: &mut C,
    session_id: &str,
) -> Result<Option<(i64, i64, i64, i64, i64)>>
where
    C: AsyncCommands + Send + Unpin,
{
    let sent_key = session_metric_key(session_id, "sent");
    let received_key = session_metric_key(session_id, "received");
    let proved_key = session_metric_key(session_id, "proved");
    let bytes_sent_key = session_metric_key(session_id, "bytes_sent");
    let bytes_rcvd_key = session_metric_key(session_id, "bytes_received");

    let (sent, received, proved, bytes_sent, bytes_received): (i64, i64, i64, i64, i64) = redis::pipe()
        .get(&sent_key)
        .get(&received_key)
        .get(&proved_key)
        .get(&bytes_sent_key)
        .get(&bytes_rcvd_key)
        .query_async(conn)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    if sent == 0 && received == 0 && proved == 0 && bytes_sent == 0 && bytes_received == 0 {
        Ok(None)
    } else {
        Ok(Some((sent, received, proved, bytes_sent, bytes_received)))
    }
}

/// Flush a batch of session counters to Postgres (including bytes)
async fn flush_batch_to_pg(
    pg: &PgPool,
    batch: &[(String, i64, i64, i64, i64, i64)],
    metrics: Option<&SessionFlusherMetrics>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut tx = pg.begin().await?;

    for (session_id, sent, received, proved, bytes_sent, bytes_received) in batch {
        sqlx::query(
            "
            UPDATE session_keys
            SET messages_sent = messages_sent + $1,
                messages_received = messages_received + $2,
                messages_proved = messages_proved + $3,
                bytes_sent = bytes_sent + $4,
                bytes_received = bytes_received + $5,
                last_used_at = NOW()
            WHERE session_id = $6 AND is_active = true
            "
        )
        .bind(sent)
        .bind(received)
        .bind(proved)
        .bind(bytes_sent)
        .bind(bytes_received)
        .bind(session_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    }

    tx.commit().await?;

    if let Some(metrics) = metrics {
        metrics
            .sessions_flushed
            .fetch_add(batch.len() as u64, Ordering::SeqCst);
    }

    Ok(())
}
