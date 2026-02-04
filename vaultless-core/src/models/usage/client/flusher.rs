//! Background flusher for periodic Redis to Postgres client metric persistence.

use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use redis::AsyncCommands;
use sqlx::{PgPool, QueryBuilder};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::usage::config::{MetricsConfig, RedisPoolType, ACTIVE_CLIENT_KEYS_SET, PROCESSING_FLAG};
use crate::models::usage::counters::{
    get_hour_window, ClientFlusherMetrics, ClientMetricCounters, ClientMetricKey,
};

// =============================================================================
// Types
// =============================================================================

/// Entry in the flush batch: ((application_id, client_id, period_start), counters, redis_key)
type BatchEntry = ((Uuid, Uuid, DateTime<Utc>), ClientMetricCounters, ClientMetricKey);

// =============================================================================
// Background Flusher
// =============================================================================

/// Starts the background flusher for client metrics.
pub fn start_client_redis_flusher(
    redis_pool: Arc<RedisPoolType>,
    pg_pool: Arc<PgPool>,
    config: Arc<MetricsConfig>,
    metrics: Option<Arc<ClientFlusherMetrics>>,
) -> (tokio::task::JoinHandle<()>, Arc<Notify>) {
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(config.flush_interval_secs));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = flush_redis_to_pg(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        false,
                    ).await {
                        error!(?e, "Client metrics flush (Redis -> PG) failed");
                        if let Some(m) = &metrics { m.record_error(); }
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received. Performing final client metrics flush...");
                    if let Err(e) = flush_redis_to_pg(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        true, // flush_all = true
                    ).await {
                        error!(?e, "Final client metrics flush failed");
                        if let Some(m) = &metrics { m.record_error(); }
                    } else {
                        info!("Final client metrics flush completed successfully.");
                    }
                    break;
                }
            }
        }
    });

    (handle, shutdown_clone)
}

// =============================================================================
// Flush Implementation
// =============================================================================

async fn flush_redis_to_pg(
    redis_pool: Arc<RedisPoolType>,
    pg: Arc<PgPool>,
    config: &MetricsConfig,
    metrics: Option<&ClientFlusherMetrics>,
    flush_all: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut conn = redis_pool.get().await.map_err(|e| VaultlessError::Internal(e.to_string()))?;

    recover_orphaned_keys(&redis_pool, config).await.unwrap_or_else(|e| {
        warn!("Failed to recover orphaned client metric keys: {}", e);
        Vec::new()
    });

    let mut cursor: u64 = 0;
    let mut batch: Vec<BatchEntry> = Vec::new();
    let mut total_keys_scanned = 0;

    let cutoff = if flush_all {
        Utc::now() + ChronoDuration::days(1)
    } else {
        get_hour_window(&Utc::now())
    };

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SSCAN")
            .arg(ACTIVE_CLIENT_KEYS_SET.as_str())
            .arg(cursor)
            .arg("COUNT").arg(1000)
            .query_async(&mut *conn)
            .await?;
        
        total_keys_scanned += keys.len();

        for key_str in keys {
            let key = match ClientMetricKey::try_from(key_str) {
                Ok(k) => k,
                Err(e) => {
                    warn!("Skipping invalid client metric key: {}", e);
                    continue;
                }
            };

            if let Some((app_id, client_id, period)) = key.parse() {
                if period >= cutoff {
                    continue;
                }

                conn.hset::<_, _, _, ()>(key.as_str(), PROCESSING_FLAG, 1).await?;
                let values: HashMap<String, i64> = conn.hgetall(key.as_str()).await?;
                
                let mut counters = ClientMetricCounters::default();
                counters.merge_from_map(&values);
                batch.push(((app_id, client_id, period), counters, key));
            } else {
                warn!("Skipping unparsable client metric key: {}", key.as_str());
            }

            if batch.len() >= config.max_batch_size {
                let to_flush = std::mem::take(&mut batch);
                flush_batch_to_pg(&pg, to_flush, &redis_pool, config, metrics).await?;
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    if !batch.is_empty() {
        flush_batch_to_pg(&pg, batch, &redis_pool, config, metrics).await?;
    }

    let duration = start.elapsed();
    info!(
        total_keys_scanned,
        duration_ms = duration.as_millis(),
        "Completed client metrics flush cycle"
    );

    if let Some(m) = metrics {
        m.total_flush_duration_ms.fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
    }
    
    Ok(())
}

async fn recover_orphaned_keys(
    redis_pool: &Arc<RedisPoolType>,
    _config: &MetricsConfig,
) -> Result<Vec<ClientMetricKey>> {
    let mut conn = redis_pool.get().await?;
    let mut orphaned = Vec::new();
    let mut cursor: u64 = 0;

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SSCAN")
            .arg(ACTIVE_CLIENT_KEYS_SET.as_str())
            .arg(cursor)
            .arg("COUNT").arg(1000)
            .query_async(&mut *conn)
            .await?;

        for key_str in keys {
            let key = ClientMetricKey::try_from(key_str)?;
            if conn.hexists::<_, _, bool>(key.as_str(), PROCESSING_FLAG).await? {
                warn!("Recovering orphaned client key: {}", key.as_str());
                conn.hdel::<_, _, ()>(key.as_str(), PROCESSING_FLAG).await?;
                orphaned.push(key);
            }
        }

        cursor = next_cursor;
        if cursor == 0 { break; }
    }
    Ok(orphaned)
}

async fn flush_batch_to_pg(
    pg: &PgPool,
    mut batch: Vec<BatchEntry>,
    redis_pool: &Arc<RedisPoolType>,
    _config: &MetricsConfig,
    metrics: Option<&ClientFlusherMetrics>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut conn = redis_pool.get().await?;
    let non_zero_batch: Vec<_> = batch.drain(..).filter(|(_, c, _)| !c.is_zero()).collect();
    if non_zero_batch.is_empty() {
        return Ok(());
    }

    let mut tx = pg.begin().await?;
    let mut qb = QueryBuilder::new(
        r#"
        INSERT INTO client_usage_metrics (
            period_start, period_end, application_id, client_id,
            messages_sent, messages_received, proofs_verified,
            total_bytes_stored, total_bytes_sent, total_bytes_received,
            rate_limit_hits
        )
        "#,
    );

    qb.push_values(non_zero_batch.iter(), |mut b, ((app_id, client_id, period_start), counters, _)| {
        let period_end = *period_start + ChronoDuration::hours(1);
        let total_bytes_stored = counters.total_bytes_sent + counters.total_bytes_received;
        b.push_bind(*period_start)
            .push_bind(period_end)
            .push_bind(*app_id)
            .push_bind(*client_id)
            .push_bind(counters.messages_sent)
            .push_bind(counters.messages_received)
            .push_bind(counters.proofs_verified)
            .push_bind(total_bytes_stored)
            .push_bind(counters.total_bytes_sent)
            .push_bind(counters.total_bytes_received)
            .push_bind(counters.rate_limit_hits);
    });
    
    qb.push(
        r#"
        ON CONFLICT (application_id, client_id, period_start) DO UPDATE SET
            messages_sent = client_usage_metrics.messages_sent + EXCLUDED.messages_sent,
            messages_received = client_usage_metrics.messages_received + EXCLUDED.messages_received,
            proofs_verified = client_usage_metrics.proofs_verified + EXCLUDED.proofs_verified,
            total_bytes_stored = client_usage_metrics.total_bytes_stored + EXCLUDED.total_bytes_stored,
            total_bytes_sent = client_usage_metrics.total_bytes_sent + EXCLUDED.total_bytes_sent,
            total_bytes_received = client_usage_metrics.total_bytes_received + EXCLUDED.total_bytes_received,
            rate_limit_hits = client_usage_metrics.rate_limit_hits + EXCLUDED.rate_limit_hits
        "#,
    );

    qb.build().execute(&mut *tx).await?;
    tx.commit().await?;

    let flushed_count = non_zero_batch.len();
    if let Some(m) = metrics {
        m.keys_processed.fetch_add(flushed_count as u64, Ordering::SeqCst);
        m.batches_processed.fetch_add(1, Ordering::SeqCst);
    }
    
    for (_, _, key) in non_zero_batch {
        conn.srem::<_, _, ()>(ACTIVE_CLIENT_KEYS_SET.as_str(), key.as_str()).await?;
        conn.del::<_, ()>(key.as_str()).await?;
    }
    
    info!(keys_flushed = flushed_count, "Flushed client metrics batch to Postgres");
    Ok(())
}
