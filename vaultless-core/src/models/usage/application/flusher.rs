//! Background flusher for periodic Redis to Postgres metric persistence.
//!
//! Handles batched upserts, orphan key recovery, and graceful shutdown.
//! Metrics are keyed by `application_id` for stability across key rotations.

use crate::error::{Result, VaultlessError};
use crate::models::app_model::material_view_helper::trigger_view_refresh_debounced;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use redis::AsyncCommands;
use sqlx::{PgPool, Postgres, query_builder::QueryBuilder};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::time::{Duration, interval};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::usage::config::{MetricsConfig, RedisPoolType, ACTIVE_KEYS_SET, PROCESSING_FLAG};
use crate::models::usage::counters::{get_hour_window, FlusherMetrics, MetricCounters, MetricKey};

// =============================================================================
// Types
// =============================================================================

/// Entry in the flush batch: ((application_id, period_start), counters, redis_key)
type BatchEntry = ((Uuid, DateTime<Utc>), MetricCounters, MetricKey);

// =============================================================================
// Background Flusher
// =============================================================================

/// Start the background flusher task
///
/// Returns a join handle and a shutdown notifier. Call `shutdown.notify_one()`
/// to trigger graceful shutdown with a final flush.
pub fn start_redis_flusher(
    redis_pool: Arc<RedisPoolType>,
    pg_pool: Arc<PgPool>,
    config: Arc<MetricsConfig>,
    metrics: Option<Arc<FlusherMetrics>>,
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
                        error!(?e, "Redis → Postgres metrics flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.record_error();
                        }
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received. Performing final metrics flush...");
                    if let Err(e) = flush_redis_to_pg(
                        Arc::clone(&redis_pool),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref),
                        true,
                    ).await {
                        error!(?e, "Final flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.record_error();
                        }
                    } else {
                        info!("Final flush completed successfully");
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

/// Main flush cycle: scan Redis, collect metrics, flush to Postgres
async fn flush_redis_to_pg(
    redis_pool: Arc<RedisPoolType>,
    pg: Arc<PgPool>,
    config: &MetricsConfig,
    metrics: Option<&FlusherMetrics>,
    flush_all: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    // Recover any orphaned keys first
    match recover_orphaned_keys(&redis_pool, config).await {
        Ok(orphaned) => {
            if !orphaned.is_empty() {
                info!("Recovered {} orphaned keys", orphaned.len());
            }
        }
        Err(e) => {
            warn!("Failed to recover orphaned keys: {}", e);
        }
    }

    // Process keys in streaming fashion
    let mut cursor: u64 = 0;
    let mut batch: Vec<BatchEntry> = Vec::new();
    let mut total_keys_scanned = 0;

    let now = Utc::now();
    let cutoff = if flush_all {
        now + ChronoDuration::days(1)
    } else {
        get_hour_window(&now)
    };

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            redis::cmd("SSCAN")
                .arg(ACTIVE_KEYS_SET.as_str())
                .arg(cursor)
                .arg("COUNT")
                .arg(1000)
                .query_async(&mut *conn),
        )
        .await
        .map_err(|_| VaultlessError::Timeout("Redis SSCAN timed out".into()))?
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        total_keys_scanned += keys.len();

        // Process this chunk immediately
        for key_str in keys {
            let key = match MetricKey::try_from(key_str) {
                Ok(key) => key,
                Err(e) => {
                    warn!("Skipping invalid metric key: {}", e);
                    continue;
                }
            };

            if let Some((application_id, period)) = key.parse() {
                if period >= cutoff {
                    continue;
                }

                // Mark as processing for idempotency
                let mark_result = tokio::time::timeout(
                    Duration::from_secs(config.redis_operation_timeout_secs),
                    conn.hset::<_, _, _, ()>(key.as_str(), PROCESSING_FLAG, 1),
                )
                .await;

                if mark_result.is_err() {
                    warn!("Timeout marking key as processing: {}", key.as_str());
                    continue;
                }

                let values_result: std::result::Result<
                    std::result::Result<HashMap<String, i64>, redis::RedisError>,
                    tokio::time::error::Elapsed,
                > = tokio::time::timeout(
                    Duration::from_secs(config.redis_operation_timeout_secs),
                    conn.hgetall(key.as_str()),
                )
                .await;

                match values_result {
                    Ok(Ok(values)) => {
                        let mut counters = MetricCounters::default();
                        counters.merge_from_map(&values);
                        batch.push(((application_id, period), counters, key));
                    }
                    Ok(Err(e)) => {
                        error!("Failed to get values for key {}: {}", key.as_str(), e);
                        if let Some(metrics) = metrics {
                            metrics.record_error();
                        }
                    }
                    Err(_) => {
                        warn!("Timeout getting values for key: {}", key.as_str());
                        if let Some(metrics) = metrics {
                            metrics.record_error();
                        }
                    }
                }
            } else {
                warn!("Skipping unparsable metric key: {}", key.as_str());
            }

            // Flush to DB if batch gets too large
            if batch.len() >= config.max_batch_size {
                let to_flush = std::mem::take(&mut batch);
                if let Err(e) = flush_batch_to_pg(&pg, to_flush, &redis_pool, config, metrics).await
                {
                    error!("Failed to flush intermediate batch: {}", e);
                    if let Some(metrics) = metrics {
                        metrics.record_error();
                    }
                }
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    // Flush any remaining items
    if !batch.is_empty() {
        flush_batch_to_pg(&pg, batch, &redis_pool, config, metrics).await?;
    }

    let duration = start.elapsed();

    info!(
        total_keys_scanned = total_keys_scanned,
        duration_ms = duration.as_millis(),
        "Completed metrics flush cycle"
    );

    if let Some(metrics) = metrics {
        metrics
            .total_flush_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
    }

    // Check if flusher is falling behind
    if duration.as_secs() > (config.flush_interval_secs / 2) {
        warn!(
            duration_secs = duration.as_secs(),
            flush_interval = config.flush_interval_secs,
            "Flusher taking more than 50% of interval! Consider scaling or increasing interval"
        );
    }

    Ok(())
}

/// Recover keys that were being processed when a previous flush was interrupted
async fn recover_orphaned_keys(
    redis_pool: &Arc<RedisPoolType>,
    config: &MetricsConfig,
) -> Result<Vec<MetricKey>> {
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let mut orphaned = Vec::new();
    let mut cursor: u64 = 0;

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            redis::cmd("SSCAN")
                .arg(ACTIVE_KEYS_SET.as_str())
                .arg(cursor)
                .arg("COUNT")
                .arg(1000)
                .query_async(&mut *conn),
        )
        .await
        .map_err(|_| VaultlessError::Timeout("Redis SSCAN timed out".into()))?
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        for key_str in keys {
            let key = match MetricKey::try_from(key_str) {
                Ok(key) => key,
                Err(e) => {
                    warn!("Invalid metric key found in tracking set: {}", e);
                    continue;
                }
            };

            let has_flag: bool = tokio::time::timeout(
                Duration::from_secs(config.redis_operation_timeout_secs),
                conn.hexists(key.as_str(), PROCESSING_FLAG),
            )
            .await
            .map_err(|_| VaultlessError::Timeout("Redis HEXISTS timed out".into()))?
            .unwrap_or(false);

            if has_flag {
                warn!("Recovering orphaned key: {}", key.as_str());
                let _ = tokio::time::timeout(
                    Duration::from_secs(config.redis_operation_timeout_secs),
                    conn.hdel::<String, &str, ()>(key.as_str(), PROCESSING_FLAG),
                )
                .await;
                orphaned.push(key);
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    Ok(orphaned)
}

/// Batch lookup of subscription IDs for applications
async fn get_subscription_ids_batch(
    pg: &PgPool,
    application_ids: &[Uuid],
) -> Result<HashMap<Uuid, Uuid>> {
    if application_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Path: applications -> users <- subscriptions (active)
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        r#"
        SELECT
            a.id as application_id,
            s.id as subscription_id
        FROM applications a
        JOIN subscriptions s ON a.user_id = s.user_id AND s.is_active = true
        WHERE a.id = ANY($1)
        "#,
    )
    .bind(application_ids)
    .fetch_all(pg)
    .await
    .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    Ok(rows.into_iter().collect())
}

/// Flush a batch of metrics to Postgres
async fn flush_batch_to_pg(
    pg: &PgPool,
    mut batch: Vec<BatchEntry>,
    redis_pool: &Arc<RedisPoolType>,
    config: &MetricsConfig,
    metrics: Option<&FlusherMetrics>,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let mut flushed_count = 0;
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    for chunk in batch.chunks(config.max_batch_size) {
        let non_zero_chunk: Vec<_> = chunk
            .iter()
            .filter(|(_, counters, _)| !counters.is_zero())
            .cloned()
            .collect();

        if non_zero_chunk.is_empty() {
            continue;
        }

        // Extract unique application IDs for batch lookup
        let application_ids: Vec<Uuid> = non_zero_chunk
            .iter()
            .map(|((application_id, _), _, _)| *application_id)
            .collect();

        // Batch lookup of subscription IDs
        let subscription_map = get_subscription_ids_batch(pg, &application_ids).await?;

        // Resolve subscription IDs and prepare data for insertion
        let mut resolved_data: Vec<(Uuid, Uuid, DateTime<Utc>, MetricCounters, MetricKey)> =
            Vec::new();
        for ((application_id, period_start), counters, key) in non_zero_chunk {
            if let Some(subscription_id) = subscription_map.get(&application_id) {
                resolved_data.push((
                    application_id,
                    *subscription_id,
                    period_start,
                    counters,
                    key,
                ));
            } else {
                error!(
                    "Could not find subscription_id for application_id: {}",
                    application_id
                );
                continue;
            }
        }

        if resolved_data.is_empty() {
            continue;
        }

        let mut tx = pg.begin().await?;

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            INSERT INTO usage_metrics (
                period_start, period_end,
                application_id, subscription_id,
                messages_sent, messages_received, proofs_verified,
                total_bytes_stored, total_bytes_sent, total_bytes_received,
                rate_limit_hits, bytes_proved, estimated_cost_cents
            )
            "#,
        );

        qb.push_values(
            resolved_data.iter(),
            |mut b, (application_id, subscription_id, period_start, counters, _)| {
                let period_end = *period_start + ChronoDuration::hours(1);
                let total_bytes_stored = counters.total_bytes_sent + counters.total_bytes_received;
                let estimated_cost = counters.estimate_cost_cents();

                b.push_bind(*period_start)
                    .push_bind(period_end)
                    .push_bind(*application_id)
                    .push_bind(*subscription_id)
                    .push_bind(counters.messages_sent)
                    .push_bind(counters.messages_received)
                    .push_bind(counters.proofs_verified)
                    .push_bind(total_bytes_stored)
                    .push_bind(counters.total_bytes_sent)
                    .push_bind(counters.total_bytes_received)
                    .push_bind(counters.rate_limit_hits)
                    .push_bind(counters.bytes_proved)
                    .push_bind(estimated_cost);
            },
        );

        // ON CONFLICT on (application_id, subscription_id, period_start)
        // Note: api_key_id is now NULL since we aggregate at application level
        qb.push(
            r#"
            ON CONFLICT (application_id, subscription_id, period_start)
            WHERE api_key_id IS NULL
            DO UPDATE SET
                messages_sent = usage_metrics.messages_sent + EXCLUDED.messages_sent,
                messages_received = usage_metrics.messages_received + EXCLUDED.messages_received,
                proofs_verified = usage_metrics.proofs_verified + EXCLUDED.proofs_verified,
                total_bytes_stored = usage_metrics.total_bytes_stored + EXCLUDED.total_bytes_stored,
                total_bytes_sent = usage_metrics.total_bytes_sent + EXCLUDED.total_bytes_sent,
                total_bytes_received = usage_metrics.total_bytes_received + EXCLUDED.total_bytes_received,
                rate_limit_hits = usage_metrics.rate_limit_hits + EXCLUDED.rate_limit_hits,
                bytes_proved = usage_metrics.bytes_proved + EXCLUDED.bytes_proved,
                estimated_cost_cents = usage_metrics.estimated_cost_cents + EXCLUDED.estimated_cost_cents
            "#,
        );

        let query = qb.build();
        query.execute(&mut *tx).await?;
        tx.commit().await?;

        flushed_count += resolved_data.len();
        if let Some(metrics) = metrics {
            metrics.batches_processed.fetch_add(1, Ordering::SeqCst);
        }
    }

    // Delete keys and remove from tracking set
    for ((_, _), _, key) in batch.drain(..) {
        if let Err(e) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            conn.srem::<_, _, ()>(ACTIVE_KEYS_SET.as_str(), key.as_str()),
        )
        .await
        {
            error!(
                "Timeout removing key {} from tracking set: {}",
                key.as_str(),
                e
            );
        }

        if let Err(e) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            conn.del::<_, ()>(key.as_str()),
        )
        .await
        {
            error!("Timeout deleting key {}: {}", key.as_str(), e);
        }
    }

    if let Some(metrics) = metrics {
        metrics
            .keys_processed
            .fetch_add(flushed_count as u64, Ordering::SeqCst);
    }

    info!(keys_flushed = flushed_count, "Flushed batch to Postgres");

    if flushed_count > 0 {
        trigger_view_refresh_debounced(Arc::new(pg.clone()), redis_pool.clone());
    }

    Ok(())
}
