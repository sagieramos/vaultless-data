//! Background flusher for periodic Redis to Postgres client metrics persistence.
//!
//! Handles batched upserts for client message counters (sent, received, proved).
//! Uses SCAN to find all client metric keys since they're keyed by client_id.

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::models::usage::config::{RedisPoolType, UsageEngineConfig};
use redis::AsyncCommands;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{Duration, interval};
use tracing::{error, info};

/// Metrics for monitoring the client flusher performance
#[derive(Debug, Default)]
pub struct ClientFlusherMetrics {
    pub clients_flushed: AtomicU64,
    pub errors: AtomicU64,
    pub total_flush_duration_ms: AtomicU64,
}

impl ClientFlusherMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }

    pub fn average_flush_duration_ms(&self) -> f64 {
        let duration = self.total_flush_duration_ms.load(Ordering::SeqCst);
        let flushed = self.clients_flushed.load(Ordering::SeqCst);
        if flushed > 0 {
            duration as f64 / flushed as f64
        } else {
            0.0
        }
    }
}

/// Start the background client flusher task
///
/// Returns a join handle and a shutdown notifier. Call `shutdown.notify_one()`
/// to trigger graceful shutdown with a final flush.
pub fn start_client_flusher(
    redis_pool: Arc<RedisPoolType>,
    pg_pool: Arc<PgPool>,
    config: Arc<UsageEngineConfig>,
    metrics: Option<Arc<ClientFlusherMetrics>>,
) -> (tokio::task::JoinHandle<()>, Arc<Notify>) {
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60)); // 60 second flush interval

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = flush_client_counters(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        false,
                    ).await {
                        error!(?e, "Client counters flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.record_error();
                        }
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received. Performing final client counters flush...");
                    if let Err(e) = flush_client_counters(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        true,
                    ).await {
                        error!(?e, "Final client flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.record_error();
                        }
                    } else {
                        info!("Final client flush completed successfully");
                    }
                    break;
                }
            }
        }
    });

    (handle, shutdown_clone)
}

/// Main flush cycle: scan Redis, collect counters, flush to Postgres
async fn flush_client_counters(
    redis_pool: Arc<RedisPoolType>,
    pg: Arc<PgPool>,
    config: &UsageEngineConfig,
    metrics: Option<&ClientFlusherMetrics>,
    flush_all: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    // Track processed client IDs to avoid duplicates in this batch
    let mut processed_clients: HashSet<String> = HashSet::new();
    // Batch format: (client_id, sent, received, proved, bytes_sent, bytes_received)
    let mut batch: Vec<(String, i64, i64, i64, i64, i64, i64)> = Vec::new();

    // SCAN for all client metric keys
    let mut cursor: u64 = 0;
    let pattern = format!("{}:*", cache_key!("metric", "client"));

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
            // Parse the client_id from the key
            // Key format: metric:client:{client_id}:{counter_type}
            let client_id = extract_client_id_from_key(&key);

            if client_id.is_empty() {
                continue;
            }

            // Skip if we already processed this client in this batch
            if !processed_clients.insert(client_id.clone()) {
                continue;
            }

            // Get counters for this client
            let counters = get_client_counters(&mut conn, &client_id).await?;

            if let Some((sent, received, proved, bytes_sent, bytes_received, bytes_proved)) = counters {
                if sent > 0 || received > 0 || proved > 0 || bytes_sent > 0 || bytes_received > 0 || bytes_proved > 0 {
                    batch.push((client_id, sent, received, proved, bytes_sent, bytes_received, bytes_proved));
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
    let clients_flushed = batch.len() as u64;

    if clients_flushed > 0 {
        info!(
            clients_flushed = clients_flushed,
            duration_ms = duration.as_millis(),
            "Completed client counters flush"
        );
    }

    if let Some(metrics) = metrics {
        metrics
            .clients_flushed
            .fetch_add(clients_flushed, Ordering::SeqCst);
        metrics
            .total_flush_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
    }

    Ok(())
}

/// Extract client_id from a Redis key
/// Key format: metric:client:{client_id}:{counter_type}
fn extract_client_id_from_key(key: &str) -> String {
    let prefix = cache_key!("metric", "client");
    if !key.starts_with(&prefix) {
        return String::new();
    }

    // Remove prefix
    let after_prefix = &key[prefix.len()..];

    // Split by : and get the client_id (first part after prefix)
    let parts: Vec<&str> = after_prefix.trim_matches(':').split(':').collect();
    if parts.is_empty() {
        return String::new();
    }

    parts[0].to_string()
}

/// Get client counters from Redis (including bytes)
async fn get_client_counters<C>(
    conn: &mut C,
    client_id: &str,
) -> Result<Option<(i64, i64, i64, i64, i64, i64)>>
where
    C: AsyncCommands + Send + Unpin,
{
    let sent_key = cache_key!("metric", "client", client_id, "sent");
    let received_key = cache_key!("metric", "client", client_id, "received");
    let proved_key = cache_key!("metric", "client", client_id, "proved");
    let bytes_sent_key = cache_key!("metric", "client", client_id, "bytes_sent");
    let bytes_rcvd_key = cache_key!("metric", "client", client_id, "bytes_received");
    let bytes_proved_key = cache_key!("metric", "client", client_id, "bytes_proved");

    let (sent, received, proved, bytes_sent, bytes_received, bytes_proved): (i64, i64, i64, i64, i64, i64) = redis::pipe()
        .get(&sent_key)
        .get(&received_key)
        .get(&proved_key)
        .get(&bytes_sent_key)
        .get(&bytes_rcvd_key)
        .get(&bytes_proved_key)
        .query_async(conn)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    if sent == 0 && received == 0 && proved == 0 && bytes_sent == 0 && bytes_received == 0 && bytes_proved == 0 {
        Ok(None)
    } else {
        Ok(Some((sent, received, proved, bytes_sent, bytes_received, bytes_proved)))
    }
}

/// Flush a batch of client counters to Postgres (including bytes)
async fn flush_batch_to_pg(
    pg: &PgPool,
    batch: &[(String, i64, i64, i64, i64, i64, i64)],
    metrics: Option<&ClientFlusherMetrics>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut tx = pg.begin().await?;

    for (client_id, sent, received, proved, bytes_sent, bytes_received, bytes_proved) in batch {
        sqlx::query(
            "
            UPDATE clients
            SET messages_sent = messages_sent + $1,
                messages_received = messages_received + $2,
                messages_proved = messages_proved + $3,
                bytes_sent = bytes_sent + $4,
                bytes_received = bytes_received + $5,
                bytes_proved = bytes_proved + $6,
                last_seen_at = NOW()
            WHERE id = $7 AND is_active = true
            "
        )
        .bind(sent)
        .bind(received)
        .bind(proved)
        .bind(bytes_sent)
        .bind(bytes_received)
        .bind(bytes_proved)
        .bind(client_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    }

    tx.commit().await?;

    if let Some(metrics) = metrics {
        metrics
            .clients_flushed
            .fetch_add(batch.len() as u64, Ordering::SeqCst);
    }

    Ok(())
}
