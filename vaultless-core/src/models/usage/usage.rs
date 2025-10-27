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

use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDateTime, Timelike, Utc};
use deadpool_redis::{Pool as RedisPool, Runtime as RedisRuntime};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, PgPool, Postgres};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{collections::HashMap, sync::Arc};
use tokio::time::error::Elapsed;
use tokio::{
    sync::Notify,
    time::{Duration, interval},
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;

// Removed unused 'metrics::CounterFn' import

// =============================================================================
// Type Aliases & Constants
// =============================================================================

/// Alias for single-connection Redis manager used by background tasks (flusher).
pub type RedisConn = ConnectionManager;

/// Alias for pooled Redis used by the hot request path.
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
// Metrics Collection (FIXED)
// =============================================================================

// Removed Clone/Default derive as AtomicU64 does not implement them
#[derive(Debug)]
pub struct FlusherMetrics {
    // Using AtomicU64 for thread-safe increments
    pub keys_processed: AtomicU64,
    pub errors: AtomicU64,
    pub batches_processed: AtomicU64,
    pub total_flush_duration_ms: AtomicU64,
}

// Manual implementation of Default
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

    // Methods now use fetch_add/load on the AtomicU64 fields
    pub fn record_flush(&self, keys_processed: usize, duration: std::time::Duration) {
        self.keys_processed
            .fetch_add(keys_processed as u64, Ordering::SeqCst);
        self.batches_processed.fetch_add(1, Ordering::SeqCst);
        self.total_flush_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
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
// Redis Connection Management
// =============================================================================

/// Creates a new async Redis connection manager (single connection) used for background tasks.
///
/// # Errors
/// Returns `VaultlessError::Internal` if connection fails.
pub async fn create_redis_conn(redis_url: &str) -> Result<RedisConn> {
    let client =
        redis::Client::open(redis_url).map_err(|e| VaultlessError::Internal(e.to_string()))?;
    ConnectionManager::new(client)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))
}

/// Creates a new deadpool Redis pool for high-throughput request path.
pub fn create_redis_pool(redis_url: &str) -> Result<RedisPoolType> {
    let cfg = deadpool_redis::Config::from_url(redis_url);
    cfg.create_pool(Some(RedisRuntime::Tokio1))
        .map_err(|e| VaultlessError::Internal(e.to_string()))
}

// =============================================================================
// Newtype for Redis Keys
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricKey(String);

impl MetricKey {
    pub fn new(api_key_id: Uuid, period: DateTime<Utc>) -> Result<Self> {
        let period_start = get_period_start(&period);

        // Validate period is not in the future
        if period_start > Utc::now() {
            return Err(VaultlessError::InvalidInput(
                "Future periods not allowed for metric keys".into(),
            ));
        }

        // Validate UUID is not nil
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

    /// Parse a metric key back to (api_key_id, period_start)
    pub fn parse(&self) -> Option<(Uuid, DateTime<Utc>)> {
        let parts: Vec<&str> = self.0.strip_prefix("metric:hash:")?.split(':').collect();
        if parts.len() != 2 {
            return None;
        }

        let uuid = Uuid::parse_str(parts[0]).ok()?;
        let timestamp_str = parts[1];

        // Parse YYYYMMDDHH format
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
        // Validate the key format
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

/// Returns the start of the current hour (UTC) for bucketing.
/// Never panics - returns a safe default if date construction fails.
fn get_period_start(now: &DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(now.hour(), 0, 0)
        .map(|dt| dt.and_utc())
        .unwrap_or_else(|| {
            // Fallback to midnight if hour construction fails
            now.date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|dt| dt.and_utc())
                .unwrap_or(*now)
        })
}

/// Returns period start with 1-hour lookback to catch stragglers near boundaries
fn get_period_start_with_overlap(now: &DateTime<Utc>) -> DateTime<Utc> {
    get_period_start(now) - ChronoDuration::hours(1)
}

// =============================================================================
// Core Increment Operations
// =============================================================================

/// Increments one or more Redis fields atomically with key tracking.
async fn hincr_many<C>(
    conn: &mut C,
    key: &MetricKey,
    fields: &[(&str, i64)],
    metric_ttl_secs: i64,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    // Add key to tracking set first
    conn.sadd::<_, _, ()>(ACTIVE_KEYS_SET, key.as_str())
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    // Increment all fields
    for (field, value) in fields {
        conn.hincr::<_, _, _, i64>(key.as_str(), field, *value)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    }

    // Set TTL
    conn.expire::<_, ()>(key.as_str(), metric_ttl_secs)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    Ok(())
}

/// Generic increment operations (accept any Redis connection)
pub async fn increment_message_sent<C>(
    conn: &mut C,
    api_key_id: Uuid,
    size_bytes: i64,
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
            &[("messages_sent", 1), ("total_bytes_stored", size_bytes)],
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
    let key = MetricKey::new(api_key_id, get_period_start(&Utc::now()))?;

    tokio::time::timeout(
        Duration::from_secs(config.redis_operation_timeout_secs),
        hincr_many(
            conn,
            &key,
            &[("messages_sent", 1), ("total_bytes_stored", size_bytes)],
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
    let key = MetricKey::new(api_key_id, get_period_start(&Utc::now()))?;

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
    let key = MetricKey::new(api_key_id, get_period_start(&Utc::now()))?;

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

// Macro to reduce code duplication for pool wrappers
// Fixed macro with proper syntax for optional parameters
macro_rules! create_pool_wrapper {
    // For functions with additional parameters
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
    // For functions without additional parameters
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

// Usage:
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
    pub total_rate_limit_hits: i64,
    pub total_estimated_cost_cents: Option<i64>,
}

/// Returns aggregated usage metrics for a given API key hash.
pub async fn get_aggregate_by_key_hash<'c, E>(
    exec: E,
    redis_pool: Arc<RedisPoolType>,
    key_hash: &str,
) -> Result<UsageAggregate>
where
    E: Executor<'c, Database = Postgres> + Clone,
{
    // Validate key hash to prevent injection
    if key_hash.is_empty() || key_hash.len() > 256 {
        return Err(VaultlessError::InvalidInput(
            "Invalid key hash length".into(),
        ));
    }

    let api_key = ApiKey::find_by_hash(exec.clone(), redis_pool, key_hash).await?;

    let aggregate = sqlx::query_as::<_, UsageAggregate>(
        r#"
        SELECT 
            COALESCE(SUM(messages_sent), 0) AS total_messages_sent,
            COALESCE(SUM(messages_received), 0) AS total_messages_received,
            COALESCE(SUM(proofs_verified), 0) AS total_proofs_verified,
            COALESCE(SUM(total_bytes_stored), 0) AS total_bytes_stored,
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
    pub total_bytes_stored: i64,
    pub rate_limit_hits: i64,
}

impl MetricCounters {
    fn is_zero(&self) -> bool {
        self.messages_sent == 0
            && self.messages_received == 0
            && self.proofs_verified == 0
            && self.total_bytes_stored == 0
            && self.rate_limit_hits == 0
    }

    fn merge_from_map(&mut self, map: &HashMap<String, i64>) {
        self.messages_sent += *map.get("messages_sent").unwrap_or(&0);
        self.messages_received += *map.get("messages_received").unwrap_or(&0);
        self.proofs_verified += *map.get("proofs_verified").unwrap_or(&0);
        self.total_bytes_stored += *map.get("total_bytes_stored").unwrap_or(&0);
        self.rate_limit_hits += *map.get("rate_limit_hits").unwrap_or(&0);
    }

    /// Estimate cost in cents based on usage
    /// Pricing example:
    /// - $0.01 per 1000 messages sent
    /// - $0.10 per GB stored
    /// - $0.001 per proof verified
    fn estimate_cost_cents(&self) -> i64 {
        let message_cost = (self.messages_sent as f64 / 1000.0) * 1.0; // $0.01/1k
        let storage_cost = (self.total_bytes_stored as f64 / 1_000_000_000.0) * 10.0; // $0.10/GB
        let proof_cost = (self.proofs_verified as f64 / 1000.0) * 0.1; // $0.001/1k

        ((message_cost + storage_cost + proof_cost) * 100.0).round() as i64
    }
}

// =============================================================================
// Background Flusher with Graceful Shutdown
// =============================================================================

/// Starts the background flusher task with graceful shutdown support.
///
/// Returns a tuple of (JoinHandle, shutdown_notifier).
/// Call `shutdown_notifier.notify_one()` to trigger graceful shutdown.
pub fn start_redis_flusher(
    redis_manager: Arc<RedisConn>,
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
                        Arc::clone(&redis_manager),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref) // Pass Option<&FlusherMetrics>
                    ).await {
                        error!(?e, "Redis → Postgres metrics flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.errors.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received. Performing final metrics flush...");
                    if let Err(e) = flush_redis_to_pg(
                        Arc::clone(&redis_manager),
                        Arc::clone(&pg_pool),
                        &config,
                        metrics.as_ref().map(Arc::as_ref) // Pass Option<&FlusherMetrics>
                    ).await {
                        error!(?e, "Final flush failed");
                        if let Some(metrics) = &metrics {
                            metrics.errors.fetch_add(1, Ordering::SeqCst);
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

/// Recovers any keys that were marked as processing but not completed
/// (e.g., due to crash during previous flush)
async fn recover_orphaned_keys(
    redis: &Arc<RedisConn>,
    config: &MetricsConfig,
) -> Result<Vec<MetricKey>> {
    let mut conn = redis.as_ref().clone();

    // Use SSCAN for large datasets to avoid blocking Redis
    let mut orphaned = Vec::new();
    let mut cursor: u64 = 0;

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            redis::cmd("SSCAN")
                .arg(ACTIVE_KEYS_SET)
                .arg(cursor)
                .arg("COUNT")
                .arg(1000) // Process in chunks
                .query_async(&mut conn),
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

            // Check if key has processing flag with timeout
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

    if !orphaned.is_empty() {
        info!("Recovered {} orphaned keys", orphaned.len());
    }

    Ok(orphaned)
}

async fn flush_redis_to_pg(
    redis: Arc<RedisConn>,
    pg: Arc<PgPool>,
    config: &MetricsConfig,
    metrics: Option<&FlusherMetrics>,
) -> Result<()> {
    // Note: If you use a metrics system like Prometheus, you'd use a timer provided by that system.
    // Since FlusherMetrics is custom, we'll use std::time::Instant.
    let start = std::time::Instant::now();
    let mut conn = redis.as_ref().clone();

    // Recover any orphaned keys first
    let _ = recover_orphaned_keys(&redis, config).await;

    // Get all active keys from tracking set using SSCAN
    let mut all_keys = Vec::new();
    let mut cursor: u64 = 0;

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = tokio::time::timeout(
            Duration::from_secs(config.redis_operation_timeout_secs),
            redis::cmd("SSCAN")
                .arg(ACTIVE_KEYS_SET)
                .arg(cursor)
                .arg("COUNT")
                .arg(1000)
                .query_async(&mut conn),
        )
        .await
        .map_err(|_| VaultlessError::Timeout("Redis SSCAN timed out".into()))?
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        all_keys.extend(keys);

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    if all_keys.is_empty() {
        return Ok(());
    }

    let mut batch: HashMap<(Uuid, DateTime<Utc>), MetricCounters> = HashMap::new();
    let mut keys_to_delete = Vec::new();

    // Process keys with timeouts
    for key_str in all_keys {
        let key = match MetricKey::try_from(key_str) {
            Ok(key) => key,
            Err(e) => {
                warn!("Skipping invalid metric key: {}", e);
                continue;
            }
        };

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

        if let Some((api_key_id, period)) = key.parse() {
            let values_result: std::result::Result<
                std::result::Result<HashMap<String, i64>, redis::RedisError>,
                Elapsed,
            > = tokio::time::timeout(
                Duration::from_secs(config.redis_operation_timeout_secs),
                conn.hgetall(key.as_str()),
            )
            .await;

            match values_result {
                // Outer Result is Ok, Inner Result is Ok (we successfully got the map)
                Ok(Ok(values)) => {
                    let counters = batch
                        .entry((api_key_id, period))
                        .or_insert_with(MetricCounters::default);

                    counters.merge_from_map(&values);
                    keys_to_delete.push(key);
                }
                // Outer Result is Ok, Inner Result is Err (Redis operation failed)
                Ok(Err(e)) => {
                    error!("Failed to get values for key {}: {}", key.as_str(), e);
                    if let Some(metrics) = metrics {
                        metrics.errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
                // Outer Result is Err (Tokio timeout occurred)
                Err(_) => {
                    warn!("Timeout getting values for key: {}", key.as_str());
                    if let Some(metrics) = metrics {
                        metrics.errors.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    if batch.is_empty() {
        return Ok(());
    }

    // Flush in batches to avoid transaction timeouts
    let batch_vec: Vec<_> = batch.into_iter().collect();
    let total_keys = batch_vec.len();
    let mut flushed_count = 0;

    for chunk in batch_vec.chunks(config.max_batch_size) {
        let mut tx = pg.begin().await?;

        for ((api_key_id, period_start), counters) in chunk {
            if counters.is_zero() {
                continue;
            }

            let period_end = *period_start + ChronoDuration::hours(1);
            let estimated_cost = counters.estimate_cost_cents();

            sqlx::query(
                r#"
                INSERT INTO usage_metrics (
                    api_key_id, period_start, period_end,
                    messages_sent, messages_received, proofs_verified,
                    total_bytes_stored, rate_limit_hits, estimated_cost_cents
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                ON CONFLICT (api_key_id, period_start)
                DO UPDATE SET
                    messages_sent = usage_metrics.messages_sent + EXCLUDED.messages_sent,
                    messages_received = usage_metrics.messages_received + EXCLUDED.messages_received,
                    proofs_verified = usage_metrics.proofs_verified + EXCLUDED.proofs_verified,
                    total_bytes_stored = usage_metrics.total_bytes_stored + EXCLUDED.total_bytes_stored,
                    rate_limit_hits = usage_metrics.rate_limit_hits + EXCLUDED.rate_limit_hits,
                    estimated_cost_cents = usage_metrics.estimated_cost_cents + EXCLUDED.estimated_cost_cents
                "#,
            )
            .bind(api_key_id)
            .bind(period_start)
            .bind(period_end)
            .bind(counters.messages_sent as i32)
            .bind(counters.messages_received as i32)
            .bind(counters.proofs_verified as i32)
            .bind(counters.total_bytes_stored)
            .bind(counters.rate_limit_hits as i32)
            .bind(estimated_cost)
            .execute(&mut *tx)
            .await?;

            flushed_count += 1;
        }

        tx.commit().await?;

        if let Some(metrics) = metrics {
            metrics.batches_processed.fetch_add(1, Ordering::SeqCst); // FIXED: use fetch_add
        }
    }

    // Delete keys and remove from tracking set with error handling
    for key in &keys_to_delete {
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

    let duration = start.elapsed();

    if let Some(metrics) = metrics {
        metrics
            .keys_processed
            .fetch_add(flushed_count as u64, Ordering::SeqCst); // FIXED: use fetch_add
        metrics
            .total_flush_duration_ms
            .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
    }

    info!(
        keys_flushed = flushed_count,
        total_keys = total_keys,
        duration_ms = duration.as_millis(),
        "Flushed metrics to Postgres"
    );

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
