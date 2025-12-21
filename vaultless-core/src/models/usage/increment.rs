//! Redis increment operations for real-time metric collection.
//!
//! These are the hot-path operations called on every API request.
//! All operations are keyed by `application_id` for stability across key rotations.

use crate::Application;
use crate::error::{Result, VaultlessError};
use chrono::Utc;
use redis::AsyncCommands;
use tokio::time::Duration;
use uuid::Uuid;

use super::config::{MetricsConfig, RedisPoolType, ACTIVE_KEYS_SET};
use super::counters::{MetricGranularity, MetricKey};

// =============================================================================
// Core Increment Operations
// =============================================================================

/// Atomically increment multiple hash fields and manage TTL
async fn hincr_many<C>(
    conn: &mut C,
    key: &MetricKey,
    fields: &[(&str, i64)],
    metric_ttl_secs: u64,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    conn.sadd::<_, _, ()>(ACTIVE_KEYS_SET.as_str(), key.as_str())
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    for (field, value) in fields {
        conn.hincr::<_, _, _, i64>(key.as_str(), field, *value)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;
    }

    conn.expire::<_, ()>(key.as_str(), metric_ttl_secs as i64)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    Ok(())
}

/// Increment message sent counter and bytes
///
/// # Arguments
/// * `conn` - Redis connection
/// * `application_id` - The application ID (stable across key rotations)
/// * `size_bytes` - Size of the message in bytes
/// * `config` - Metrics configuration
pub async fn increment_message_sent<C>(
    conn: &mut C,
    application_id: Uuid,
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

    // Real-time quota tracking (keyed by application_id)
    let monthly_key = Application::quota_key(application_id);
    let ttl_seconds: i64 = 31 * 24 * 60 * 60; // ~31 days

    // Atomically increment the monthly key and set TTL only if it's new
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

    let now = Utc::now();
    let key = MetricKey::new(application_id, now, MetricGranularity::Hour)?;

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

/// Increment message received counter and bytes
pub async fn increment_message_received<C>(
    conn: &mut C,
    application_id: Uuid,
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
    let key = MetricKey::new(application_id, now, MetricGranularity::Hour)?;

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

/// Increment proof verified counter
pub async fn increment_proof_verified<C>(
    conn: &mut C,
    application_id: Uuid,
    config: &MetricsConfig,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let now = Utc::now();
    let key = MetricKey::new(application_id, now, MetricGranularity::Hour)?;

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

/// Increment rate limit hit counter
pub async fn increment_rate_limit_hit<C>(
    conn: &mut C,
    application_id: Uuid,
    config: &MetricsConfig,
) -> Result<()>
where
    C: AsyncCommands + Send + Unpin,
{
    let now = Utc::now();
    let key = MetricKey::new(application_id, now, MetricGranularity::Hour)?;

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
            application_id: Uuid,
            $arg_name: $arg_type,
            config: &MetricsConfig,
        ) -> Result<()> {
            let mut conn = pool
                .get()
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            $inner(&mut conn, application_id, $arg_name, config).await
        }
    };
    ($name:ident, $inner:ident) => {
        pub async fn $name(
            pool: &RedisPoolType,
            application_id: Uuid,
            config: &MetricsConfig,
        ) -> Result<()> {
            let mut conn = pool
                .get()
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            $inner(&mut conn, application_id, config).await
        }
    };
}

create_pool_wrapper!(increment_message_sent_pool, increment_message_sent, size_bytes: i64);
create_pool_wrapper!(increment_message_received_pool, increment_message_received, size_bytes: i64);
create_pool_wrapper!(increment_proof_verified_pool, increment_proof_verified);
create_pool_wrapper!(increment_rate_limit_hit_pool, increment_rate_limit_hit);
