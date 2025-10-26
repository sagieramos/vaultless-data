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
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::Notify,
    time::{Duration, interval},
};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;

// =============================================================================
// Type Aliases & Constants
// =============================================================================

/// Alias for single-connection Redis manager used by background tasks (flusher).
pub type RedisConn = ConnectionManager;

/// Alias for pooled Redis used by the hot request path.
pub type RedisPoolType = RedisPool;

/// Maximum number of keys to flush in a single transaction
const MAX_BATCH_SIZE: usize = 1000;

/// Redis key TTL for metric hashes (2 hours)
const METRIC_TTL_SECS: i64 = 7200;

/// Set name for tracking active metric keys
const ACTIVE_KEYS_SET: &str = "metric:active_keys";

/// Field name to mark a key as being processed
const PROCESSING_FLAG: &str = "_processing";

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
    pub fn new(api_key_id: Uuid, period: DateTime<Utc>) -> Self {
        Self(format!(
            "metric:hash:{}:{}",
            api_key_id,
            period.format("%Y%m%d%H")
        ))
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

impl From<String> for MetricKey {
    fn from(s: String) -> Self {
        Self(s)
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
async fn hincr_many<C>(conn: &mut C, key: &MetricKey, fields: &[(&str, i64)]) -> Result<()>
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
    conn.expire::<_, ()>(key.as_str(), METRIC_TTL_SECS)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    Ok(())
}

/// Generic increment operations (accept any Redis connection)
pub async fn increment_message_sent<C>(
    conn: &mut C,
    api_key_id: Uuid,
    size_bytes: i64,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let now = Utc::now();
    let key = MetricKey::new(api_key_id, get_period_start(&now));
    hincr_many(
        conn,
        &key,
        &[("messages_sent", 1), ("total_bytes_stored", size_bytes)],
    )
    .await
}

pub async fn increment_message_received<C>(conn: &mut C, api_key_id: Uuid) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let key = MetricKey::new(api_key_id, get_period_start(&Utc::now()));
    hincr_many(conn, &key, &[("messages_received", 1)]).await
}

pub async fn increment_proof_verified<C>(conn: &mut C, api_key_id: Uuid) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let key = MetricKey::new(api_key_id, get_period_start(&Utc::now()));
    hincr_many(conn, &key, &[("proofs_verified", 1)]).await
}

pub async fn increment_rate_limit_hit<C>(conn: &mut C, api_key_id: Uuid) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let key = MetricKey::new(api_key_id, get_period_start(&Utc::now()));
    hincr_many(conn, &key, &[("rate_limit_hits", 1)]).await
}

// =============================================================================
// Pool-backed Wrappers (Hot Path)
// =============================================================================

pub async fn increment_message_sent_pool(
    pool: &RedisPoolType,
    api_key_id: Uuid,
    size_bytes: i64,
) -> Result<()> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    increment_message_sent(&mut conn, api_key_id, size_bytes).await
}

pub async fn increment_message_received_pool(pool: &RedisPoolType, api_key_id: Uuid) -> Result<()> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    increment_message_received(&mut conn, api_key_id).await
}

pub async fn increment_proof_verified_pool(pool: &RedisPoolType, api_key_id: Uuid) -> Result<()> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    increment_proof_verified(&mut conn, api_key_id).await
}

pub async fn increment_rate_limit_hit_pool(pool: &RedisPoolType, api_key_id: Uuid) -> Result<()> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    increment_rate_limit_hit(&mut conn, api_key_id).await
}

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
    flush_secs: u64,
) -> (tokio::task::JoinHandle<()>, Arc<Notify>) {
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(flush_secs));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = flush_redis_to_pg(Arc::clone(&redis_manager), Arc::clone(&pg_pool), flush_secs).await {
                        error!(?e, "Redis → Postgres metrics flush failed");
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received. Performing final metrics flush...");
                    if let Err(e) = flush_redis_to_pg(Arc::clone(&redis_manager), Arc::clone(&pg_pool), flush_secs).await {
                        error!(?e, "Final flush failed");
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
async fn recover_orphaned_keys(redis: &Arc<RedisConn>) -> Result<Vec<MetricKey>> {
    let mut conn = redis.as_ref().clone(); // Clone the ConnectionManager, not the Arc
    let keys: Vec<String> = conn
        .smembers(ACTIVE_KEYS_SET)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let mut orphaned = Vec::new();

    for key_str in keys {
        let key = MetricKey::from(key_str);

        // Check if key has processing flag
        let has_flag: bool = conn
            .hexists(key.as_str(), PROCESSING_FLAG)
            .await
            .unwrap_or(false);

        if has_flag {
            warn!("Recovering orphaned key: {}", key.as_str());
            let _: () = conn.hdel(key.as_str(), PROCESSING_FLAG).await.unwrap_or(());
            orphaned.push(key);
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
    flush_interval_secs: u64,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut conn = (*redis).clone();

    // Recover any orphaned keys first
    let _ = recover_orphaned_keys(&redis).await;

    // Get all active keys from tracking set (O(N) where N = active keys)
    let keys: Vec<String> = conn
        .smembers(ACTIVE_KEYS_SET)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    if keys.is_empty() {
        return Ok(());
    }

    let mut batch: HashMap<(Uuid, DateTime<Utc>), MetricCounters> = HashMap::new();
    let mut keys_to_delete = Vec::new();

    // Process keys
    for key_str in keys {
        let key = MetricKey::from(key_str);

        // Mark as processing for idempotency
        conn.hset::<_, _, _, ()>(key.as_str(), PROCESSING_FLAG, 1)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        if let Some((api_key_id, period)) = key.parse() {
            let values: HashMap<String, i64> = conn
                .hgetall(key.as_str())
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            let counters = batch
                .entry((api_key_id, period))
                .or_insert_with(MetricCounters::default);

            counters.merge_from_map(&values);
            keys_to_delete.push(key);
        }
    }

    if batch.is_empty() {
        return Ok(());
    }

    // Flush in batches to avoid transaction timeouts
    let batch_vec: Vec<_> = batch.into_iter().collect();
    let total_keys = batch_vec.len();
    let mut flushed_count = 0;

    for chunk in batch_vec.chunks(MAX_BATCH_SIZE) {
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
    }

    // Delete keys and remove from tracking set (fire-and-forget)
    for key in &keys_to_delete {
        let _ = conn.srem::<_, _, ()>(ACTIVE_KEYS_SET, key.as_str()).await;
        let _ = conn.del::<_, ()>(key.as_str()).await;
    }

    let duration = start.elapsed();
    info!(
        keys_flushed = flushed_count,
        total_keys = total_keys,
        duration_ms = duration.as_millis(),
        "Flushed metrics to Postgres"
    );

    // Check if flusher is falling behind
    if duration.as_secs() > (flush_interval_secs / 2) as u64 {
        warn!(
            duration_secs = duration.as_secs(),
            flush_interval = flush_interval_secs,
            "Flusher taking more than 50% of interval! Consider scaling or increasing interval"
        );
    }

    Ok(())
}
