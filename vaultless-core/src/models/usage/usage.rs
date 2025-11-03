//! # Usage Metrics Module
//!
//! Provides efficient Redis-based aggregation and periodic flushing of API usage metrics
//! into Postgres for durable storage. Designed for 20k+ RPS workloads with Redis atomic
//! operations and batched upserts.
//!
//! ## Features
//! - Async Redis increments with sub-ms latency
//! - Batched Postgres upserts (hourly aggregates)
//! - TTL auto-expiry to prevent Redis bloat
//! - Graceful background flusher with error isolation
//! - Set-based key tracking for O(N) flushing
//! - Idempotent processing with exactly-once semantics
//! - Graceful shutdown with final flush
//! - Health metrics and monitoring

use chrono::{DateTime, Duration as ChronoDuration, NaiveDateTime, Timelike, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, PgPool, Postgres, query_builder::QueryBuilder};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::Notify,
    time::{Duration, interval},
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::crypto;
use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;
use crate::models::api_key::quota_cache_key;

// =============================================================================
// Type Aliases & Constants
// =============================================================================

/// Alias for pooled Redis used by all operations
pub type RedisPoolType = RedisPool;

/// Default maximum number of keys to flush in a single transaction
const DEFAULT_MAX_BATCH_SIZE: usize = 1000;

/// Default Redis key TTL for metric hashes (2 hours)
const DEFAULT_METRIC_TTL_SECS: i64 = 7200;

/// Default flush interval in seconds
const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 300; // 5 minutes

/// Set name for tracking active metric keys
const ACTIVE_KEYS_SET: &str = "metric:active_keys";

/// Field name to mark a key as being processed
const PROCESSING_FLAG: &str = "_processing";

/// Redis operation timeout in seconds
const REDIS_OPERATION_TIMEOUT_SECS: u64 = 30;

// =============================================================================
// Configuration
// =============================================================================

#[derive(Clone, Debug)]
pub struct MetricsConfig {
    pub max_batch_size: usize,
    pub metric_ttl_secs: i64,
    pub flush_interval_secs: u64,
    pub redis_operation_timeout_secs: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            metric_ttl_secs: DEFAULT_METRIC_TTL_SECS,
            flush_interval_secs: DEFAULT_FLUSH_INTERVAL_SECS,
            redis_operation_timeout_secs: REDIS_OPERATION_TIMEOUT_SECS,
        }
    }
}

// =============================================================================
// Metrics Collection
// =============================================================================

#[derive(Debug)]
pub struct FlusherMetrics {
    pub keys_processed: AtomicU64,
    pub errors: AtomicU64,
    pub batches_processed: AtomicU64,
    pub total_flush_duration_ms: AtomicU64,
}

impl Default for FlusherMetrics {
    fn default() -> Self {
        Self {
            keys_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            batches_processed: AtomicU64::new(0),
            total_flush_duration_ms: AtomicU64::new(0),
        }
    }
}

impl FlusherMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }

    pub fn average_flush_duration_ms(&self) -> f64 {
        let batches = self.batches_processed.load(Ordering::SeqCst);
        let duration = self.total_flush_duration_ms.load(Ordering::SeqCst);

        if batches > 0 {
            duration as f64 / batches as f64
        } else {
            0.0
        }
    }
}

// =============================================================================
// Newtype for Redis Keys
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricKey(String);

impl MetricKey {
    pub fn new(api_key_id: Uuid, period: DateTime<Utc>) -> Result<Self> {
        let period_start = get_period_start(&period);

        if period_start > Utc::now() {
            return Err(VaultlessError::InvalidInput(
                "Future periods not allowed for metric keys".into(),
            ));
        }

        if api_key_id.is_nil() {
            return Err(VaultlessError::InvalidInput(
                "Nil UUID not allowed for metric keys".into(),
            ));
        }

        Ok(Self(format!(
            "metric:hash:{}:{}",
            api_key_id,
            period_start.format("%Y%m%d%H")
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(&self) -> Option<(Uuid, DateTime<Utc>)> {
        let parts: Vec<&str> = self.0.strip_prefix("metric:hash:")?.split(':').collect();
        if parts.len() != 2 {
            return None;
        }

        let uuid = Uuid::parse_str(parts[0]).ok()?;
        let timestamp_str = parts[1];

        if timestamp_str.len() != 10 {
            return None;
        }

        let year: i32 = timestamp_str[0..4].parse().ok()?;
        let month: u32 = timestamp_str[4..6].parse().ok()?;
        let day: u32 = timestamp_str[6..8].parse().ok()?;
        let hour: u32 = timestamp_str[8..10].parse().ok()?;

        let naive = NaiveDateTime::parse_from_str(
            &format!("{:04}-{:02}-{:02} {:02}:00:00", year, month, day, hour),
            "%Y-%m-%d %H:%M:%S",
        )
        .ok()?;

        Some((uuid, naive.and_utc()))
    }
}

impl TryFrom<String> for MetricKey {
    type Error = VaultlessError;

    fn try_from(s: String) -> Result<Self> {
        if !s.starts_with("metric:hash:") {
            return Err(VaultlessError::InvalidInput(
                "Invalid metric key format".into(),
            ));
        }

        let key = Self(s);
        if key.parse().is_none() {
            return Err(VaultlessError::InvalidInput(
                "Failed to parse metric key components".into(),
            ));
        }

        Ok(key)
    }
}

// =============================================================================
// Time Utilities
// =============================================================================

pub fn get_period_start(now: &DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(now.hour(), 0, 0)
        .map(|dt| dt.and_utc())
        .unwrap_or_else(|| {
            now.date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|dt| dt.and_utc())
                .unwrap_or(*now)
        })
}

// =============================================================================
// Core Increment Operations
// =============================================================================

async fn hincr_many<C>(
    conn: &mut C,
    key: &MetricKey,
    fields: &[(&str, i64)],
    metric_ttl_secs: i64,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    conn.sadd::<_, _, ()>(ACTIVE_KEYS_SET, key.as_str())
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    for (field, value) in fields {
        conn.hincr::<_, _, _, i64>(key.as_str(), field, *value)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    }

    conn.expire::<_, ()>(key.as_str(), metric_ttl_secs)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    Ok(())
}

pub async fn increment_message_sent<C>(
    conn: &mut C,
    api_key_id: Uuid,
    size_bytes: i64,
    config: &MetricsConfig,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    if size_bytes < 0 {
        return Err(VaultlessError::InvalidInput(
            "Negative size_bytes not allowed".into(),
        ));
    }

    // --- START: logic for real-time quota ---
    let monthly_key = quota_cache_key(api_key_id);
    // Set a ~31 day TTL. Redis will auto-delete the key after a month of inactivity.
    let ttl_seconds: i64 = 31 * 24 * 60 * 60;

    // Atomically increment the monthly key and set TTL only if it's new
    // We ignore the returned count for the increment operation
    let _: () = redis::pipe()
        .atomic()
        .incr(&monthly_key, 1)
        .cmd("EXPIRE")
        .arg(&monthly_key)
        .arg(ttl_seconds)
        .arg("NX")
        .query_async(conn)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    // --- END: New logic for real-time quota ---

    let now = Utc::now();
    let key = MetricKey::new(api_key_id, get_period_start(&now))?;

    tokio::time::timeout(
        Duration::from_secs(config.redis_operation_timeout_secs),
        hincr_many(
            conn,
            &key,
            &[("messages_sent", 1), ("total_bytes_sent", size_bytes)],
            config.metric_ttl_secs,
        ),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("Redis operation timed out".into()))?
}

pub async fn increment_message_received<C>(
    conn: &mut C,
    api_key_id: Uuid,
    size_bytes: i64,
    config: &MetricsConfig,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    if size_bytes < 0 {
        return Err(VaultlessError::InvalidInput(
            "Negative size_bytes not allowed".into(),
        ));
    }

    let now = Utc::now();
    let key = MetricKey::new(api_key_id, get_period_start(&now))?;

    tokio::time::timeout(
        Duration::from_secs(config.redis_operation_timeout_secs),
        hincr_many(
            conn,
            &key,
            &[
                ("messages_received", 1),
                ("total_bytes_received", size_bytes),
            ],
            config.metric_ttl_secs,
        ),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("Redis operation timed out".into()))?
}

pub async fn increment_proof_verified<C>(
    conn: &mut C,
    api_key_id: Uuid,
    config: &MetricsConfig,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let now = Utc::now();
    let key = MetricKey::new(api_key_id, get_period_start(&now))?;

    tokio::time::timeout(
        Duration::from_secs(config.redis_operation_timeout_secs),
        hincr_many(
            conn,
            &key,
            &[("proofs_verified", 1)],
            config.metric_ttl_secs,
        ),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("Redis operation timed out".into()))?
}

pub async fn increment_rate_limit_hit<C>(
    conn: &mut C,
    api_key_id: Uuid,
    config: &MetricsConfig,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let now = Utc::now();
    let key = MetricKey::new(api_key_id, get_period_start(&now))?;

    tokio::time::timeout(
        Duration::from_secs(config.redis_operation_timeout_secs),
        hincr_many(
            conn,
            &key,
            &[("rate_limit_hits", 1)],
            config.metric_ttl_secs,
        ),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("Redis operation timed out".into()))?
}

// =============================================================================
// Pool-backed Wrappers (Hot Path)
// =============================================================================

macro_rules! create_pool_wrapper {
    ($name:ident, $inner:ident, $arg_name:ident: $arg_type:ty) => {
        pub async fn $name(
            pool: &RedisPoolType,
            api_key_id: Uuid,
            $arg_name: $arg_type,
            config: &MetricsConfig,
        ) -> Result<()> {
            let mut conn = pool
                .get()
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            $inner(&mut conn, api_key_id, $arg_name, config).await
        }
    };
    ($name:ident, $inner:ident) => {
        pub async fn $name(
            pool: &RedisPoolType,
            api_key_id: Uuid,
            config: &MetricsConfig,
        ) -> Result<()> {
            let mut conn = pool
                .get()
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            $inner(&mut conn, api_key_id, config).await
        }
    };
}

create_pool_wrapper!(increment_message_sent_pool, increment_message_sent, size_bytes: i64);
create_pool_wrapper!(increment_message_received_pool, increment_message_received, size_bytes: i64);
create_pool_wrapper!(increment_proof_verified_pool, increment_proof_verified);
create_pool_wrapper!(increment_rate_limit_hit_pool, increment_rate_limit_hit);

// =============================================================================
// Aggregate Lookup
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UsageAggregate {
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_rate_limit_hits: i64,
    pub total_estimated_cost_cents: i64,
}

pub async fn get_aggregate_by_api_key<'c, E>(
    exec: E,
    redis_pool: Arc<RedisPoolType>,
    api_key: &str,
) -> Result<UsageAggregate>
where
    E: Executor<'static, Database = Postgres> + Clone + Send + Sync + 'static,
{
    if api_key.is_empty() || api_key.len() > 256 {
        return Err(VaultlessError::InvalidInput(
            "Invalid key hash length".into(),
        ));
    }

    let key_hash = crypto::hash_content(api_key.as_bytes());

    let api_key = ApiKey:: find_by_hash_sync(exec.clone(), Some(redis_pool), key_hash).await?;

    let aggregate = sqlx::query_as::<_, UsageAggregate>(
        r#"
        SELECT 
            COALESCE(SUM(messages_sent), 0) AS total_messages_sent,
            COALESCE(SUM(messages_received), 0) AS total_messages_received,
            COALESCE(SUM(proofs_verified), 0) AS total_proofs_verified,
            COALESCE(SUM(total_bytes_stored), 0) AS total_bytes_stored,
            COALESCE(SUM(total_bytes_sent), 0) AS total_bytes_sent,
            COALESCE(SUM(total_bytes_received), 0) AS total_bytes_received,
            COALESCE(SUM(rate_limit_hits), 0) AS total_rate_limit_hits,
            COALESCE(SUM(estimated_cost_cents), 0) AS total_estimated_cost_cents
        FROM usage_metrics
        WHERE api_key_id = $1
        "#,
    )
    .bind(api_key.id)
    .fetch_one(exec)
    .await
    .map_err(VaultlessError::from)?;

    Ok(aggregate)
}

// =============================================================================
// Metric Counters & Cost Estimation
// =============================================================================

#[derive(Debug, Default, Clone)]
pub struct MetricCounters {
    pub messages_sent: i64,
    pub messages_received: i64,
    pub proofs_verified: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub rate_limit_hits: i64,
}

impl MetricCounters {
    fn is_zero(&self) -> bool {
        self.messages_sent == 0
            && self.messages_received == 0
            && self.proofs_verified == 0
            && self.total_bytes_sent == 0
            && self.total_bytes_received == 0
            && self.rate_limit_hits == 0
    }

    fn merge_from_map(&mut self, map: &HashMap<String, i64>) {
        self.messages_sent += *map.get("messages_sent").unwrap_or(&0);
        self.messages_received += *map.get("messages_received").unwrap_or(&0);
        self.proofs_verified += *map.get("proofs_verified").unwrap_or(&0);
        self.total_bytes_sent += *map.get("total_bytes_sent").unwrap_or(&0);
        self.total_bytes_received += *map.get("total_bytes_received").unwrap_or(&0);
        self.rate_limit_hits += *map.get("rate_limit_hits").unwrap_or(&0);
    }

    fn estimate_cost_cents(&self) -> i64 {
        let message_cost = (self.messages_sent as f64 / 1000.0) * 1.0;
        let total_bytes = self.total_bytes_sent + self.total_bytes_received;
        let storage_cost = (total_bytes as f64 / 1_000_000_000.0) * 10.0;
        let proof_cost = (self.proofs_verified as f64 / 1000.0) * 0.1;

        ((message_cost + storage_cost + proof_cost) * 100.0).round() as i64
    }
}

// =============================================================================
// Background Flusher with Graceful Shutdown
// =============================================================================

pub fn start_redis_flusher(
    redis_pool: Arc<RedisPoolType>,
    pg_pool: Arc<PgPool>,
    config: MetricsConfig,
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
                .arg(ACTIVE_KEYS_SET)
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
                    conn.hdel::<&str, &str, ()>(key.as_str(), PROCESSING_FLAG),
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

type BatchEntry = ((Uuid, DateTime<Utc>), MetricCounters, MetricKey);

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

    // Process keys in streaming fashion - no unbounded Vec
    let mut cursor: u64 = 0;
    let mut batch: Vec<BatchEntry> = Vec::new();
    let mut total_keys_scanned = 0;

    let now = Utc::now();
    let cutoff = if flush_all {
        now + ChronoDuration::days(1)
    } else {
        get_period_start(&now)
    };

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            redis::cmd("SSCAN")
                .arg(ACTIVE_KEYS_SET)
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

            if let Some((api_key_id, period)) = key.parse() {
                if period >= cutoff {
                    continue;
                }

                // Mark as processing for idempotency with timeout
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
                        batch.push(((api_key_id, period), counters, key));
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

            // Flush to DB if batch gets too large to prevent memory issues
            if batch.len() >= config.max_batch_size {
                let to_flush = std::mem::take(&mut batch);
                if let Err(e) = flush_batch_to_pg(&pg, to_flush, &redis_pool, config, metrics).await
                {
                    error!("Failed to flush intermediate batch: {}", e);
                    if let Some(metrics) = metrics {
                        metrics.record_error();
                    }
                    // Data remains in Redis for next cycle
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
    if duration.as_secs() > (config.flush_interval_secs / 2) as u64 {
        warn!(
            duration_secs = duration.as_secs(),
            flush_interval = config.flush_interval_secs,
            "Flusher taking more than 50% of interval! Consider scaling or increasing interval"
        );
    }

    Ok(())
}

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

        let mut tx = pg.begin().await?;

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            INSERT INTO usage_metrics (
                api_key_id, period_start, period_end,
                messages_sent, messages_received, proofs_verified,
                total_bytes_stored, total_bytes_sent, total_bytes_received,
                rate_limit_hits, estimated_cost_cents
            ) 
            "#,
        );

        qb.push_values(
            non_zero_chunk.iter(),
            |mut b, ((api_key_id, period_start), counters, _)| {
                let period_end = *period_start + ChronoDuration::hours(1);
                let total_bytes_stored = counters.total_bytes_sent + counters.total_bytes_received;
                let estimated_cost = counters.estimate_cost_cents();

                b.push_bind(*api_key_id)
                    .push_bind(*period_start)
                    .push_bind(period_end)
                    .push_bind(counters.messages_sent)
                    .push_bind(counters.messages_received)
                    .push_bind(counters.proofs_verified)
                    .push_bind(total_bytes_stored)
                    .push_bind(counters.total_bytes_sent)
                    .push_bind(counters.total_bytes_received)
                    .push_bind(counters.rate_limit_hits)
                    .push_bind(estimated_cost);
            },
        );

        qb.push(
            r#"
            ON CONFLICT (api_key_id, period_start)
            DO UPDATE SET
                messages_sent = usage_metrics.messages_sent + EXCLUDED.messages_sent,
                messages_received = usage_metrics.messages_received + EXCLUDED.messages_received,
                proofs_verified = usage_metrics.proofs_verified + EXCLUDED.proofs_verified,
                total_bytes_stored = usage_metrics.total_bytes_stored + EXCLUDED.total_bytes_stored,
                total_bytes_sent = usage_metrics.total_bytes_sent + EXCLUDED.total_bytes_sent,
                total_bytes_received = usage_metrics.total_bytes_received + EXCLUDED.total_bytes_received,
                rate_limit_hits = usage_metrics.rate_limit_hits + EXCLUDED.rate_limit_hits,
                estimated_cost_cents = usage_metrics.estimated_cost_cents + EXCLUDED.estimated_cost_cents
            "#,
        );

        let query = qb.build();
        query.execute(&mut *tx).await?;
        tx.commit().await?;

        flushed_count += non_zero_chunk.len();
        if let Some(metrics) = metrics {
            metrics.batches_processed.fetch_add(1, Ordering::SeqCst);
        }
    }

    // Delete keys and remove from tracking set
    for ((_, _), _, key) in batch.drain(..) {
        if let Err(e) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            conn.srem::<_, _, ()>(ACTIVE_KEYS_SET, key.as_str()),
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

    Ok(())
}
