//! Redis state restoration from Postgres.
//!
//! Restores missing or recent aggregated hourly rows from Postgres into Redis
//! so a Redis restart can continue from the last persisted state.

use crate::models::usage::config::{RedisPoolType, ACTIVE_KEYS_SET};
use crate::models::usage::counters::{MetricGranularity, AppMetricKey};
use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Lookback period in seconds (48 hours)
const LOOKBACK_SECS: i64 = 48 * 3600;

/// Restore missing or recent aggregated hourly rows from Postgres into Redis.
///
/// This function handles two scenarios:
/// 1. Redis is empty: Restore all usage from the last 48 hours
/// 2. Redis has data: Only restore periods newer than what's already tracked
pub async fn restore_recent_or_missing_periods_from_pg(
    redis_pool: &Arc<RedisPoolType>,
    pg: Arc<PgPool>,
) -> Result<()> {
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    // 1. Get all active metric keys from Redis
    let active_keys: Vec<String> = redis::cmd("SMEMBERS")
        .arg(ACTIVE_KEYS_SET.as_str())
        .query_async(&mut *conn)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    if active_keys.is_empty() {
        // Redis empty: fallback to last N hours
        tracing::info!(
            "No active keys in Redis, restoring last {} hours from Postgres",
            LOOKBACK_SECS / 3600
        );

        // Get all usage metrics for the lookback period, grouped by application
        let rows = sqlx::query(
            r#"
            SELECT application_id, period_start,
                   COALESCE(messages_sent, 0) as messages_sent,
                   COALESCE(messages_received, 0) as messages_received,
                   COALESCE(proofs_verified, 0) as proofs_verified,
                   COALESCE(total_bytes_sent, 0) as total_bytes_sent,
                   COALESCE(total_bytes_received, 0) as total_bytes_received,
                   COALESCE(rate_limit_hits, 0) as rate_limit_hits
            FROM usage_metrics
            WHERE period_start >= (now() - ($1 || ' seconds')::interval)
            "#,
        )
        .bind(LOOKBACK_SECS)
        .fetch_all(pg.as_ref())
        .await
        .map_err(VaultlessError::from)?;

        // Restore each row into Redis
        for r in &rows {
            let application_id: Uuid = r
                .try_get("application_id")
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            let period_start: DateTime<Utc> = r
                .try_get("period_start")
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            let metric_key = AppMetricKey::new(application_id, period_start, MetricGranularity::Hour)?;
            let key_str = metric_key.as_str();

            let mut pipe = redis::pipe();
            pipe.atomic()
                .hset(
                    &key_str,
                    "messages_sent",
                    r.try_get::<i64, _>("messages_sent").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "messages_received",
                    r.try_get::<i64, _>("messages_received").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "proofs_verified",
                    r.try_get::<i64, _>("proofs_verified").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "total_bytes_sent",
                    r.try_get::<i64, _>("total_bytes_sent").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "total_bytes_received",
                    r.try_get::<i64, _>("total_bytes_received").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "rate_limit_hits",
                    r.try_get::<i64, _>("rate_limit_hits").unwrap_or(0),
                )
                .sadd(ACTIVE_KEYS_SET.as_str(), &key_str)
                .expire(&key_str, LOOKBACK_SECS);
            let _: () = pipe
                .query_async(&mut *conn)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
        }

        tracing::info!("Restored {} usage keys into Redis (fallback)", rows.len());
        return Ok(());
    }

    // 2. Redis has active keys: restore only missing periods
    let mut last_period_per_app: HashMap<Uuid, DateTime<Utc>> = HashMap::new();
    for key_str in active_keys {
        if let Ok(key) = AppMetricKey::try_from(key_str.to_string())
            && let Some((application_id, period_start)) = key.parse()
        {
            last_period_per_app
                .entry(application_id)
                .and_modify(|existing| {
                    if period_start > *existing {
                        *existing = period_start;
                    }
                })
                .or_insert(period_start);
        }
    }

    // For each application, restore any periods newer than what we have
    for (application_id, last_period) in last_period_per_app {
        let rows = sqlx::query(
            r#"
            SELECT application_id, period_start,
                   COALESCE(messages_sent, 0) as messages_sent,
                   COALESCE(messages_received, 0) as messages_received,
                   COALESCE(proofs_verified, 0) as proofs_verified,
                   COALESCE(total_bytes_sent, 0) as total_bytes_sent,
                   COALESCE(total_bytes_received, 0) as total_bytes_received,
                   COALESCE(rate_limit_hits, 0) as rate_limit_hits
            FROM usage_metrics
            WHERE application_id = $1
              AND period_start > $2
            "#,
        )
        .bind(application_id)
        .bind(last_period)
        .fetch_all(pg.as_ref())
        .await
        .map_err(VaultlessError::from)?;

        for r in rows {
            let period_start: DateTime<Utc> = r
                .try_get("period_start")
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            let metric_key = AppMetricKey::new(application_id, period_start, MetricGranularity::Hour)?;
            let key_str = metric_key.as_str();

            let mut pipe = redis::pipe();
            pipe.atomic()
                .hset(
                    &key_str,
                    "messages_sent",
                    r.try_get::<i64, _>("messages_sent").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "messages_received",
                    r.try_get::<i64, _>("messages_received").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "proofs_verified",
                    r.try_get::<i64, _>("proofs_verified").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "total_bytes_sent",
                    r.try_get::<i64, _>("total_bytes_sent").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "total_bytes_received",
                    r.try_get::<i64, _>("total_bytes_received").unwrap_or(0),
                )
                .hset(
                    &key_str,
                    "rate_limit_hits",
                    r.try_get::<i64, _>("rate_limit_hits").unwrap_or(0),
                )
                .sadd(ACTIVE_KEYS_SET.as_str(), &key_str)
                .expire(&key_str, LOOKBACK_SECS);
            let _: () = pipe
                .query_async(&mut *conn)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
        }
    }

    Ok(())
}
