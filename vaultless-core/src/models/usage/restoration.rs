use super::usage::{ACTIVE_KEYS_SET, MetricKey, RedisPoolType};
use crate::error::{Result, VaultlessError};
const LOOKBACK_HOUR: i64 = 48;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::Row;
use std::collections::HashMap;
// add where sqlx::Row is available for ad-hoc queries
use std::sync::Arc;
use uuid::Uuid;
/// Restore missing or recent aggregated hourly rows from Postgres into Redis
/// so a Redis restart can continue from the last persisted state.
///
/// - config.metric_restore_lookback_hours: how many hours back to restore if Redis is empty
pub async fn restore_recent_or_missing_periods_from_pg(
    redis_pool: &Arc<RedisPoolType>,
    pg: &Arc<PgPool>,
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
            LOOKBACK_HOUR
        );

        let rows = sqlx::query(
            r#"
            SELECT api_key_id, period_start,
                   COALESCE(messages_sent,0) as messages_sent,
                   COALESCE(messages_received,0) as messages_received,
                   COALESCE(proofs_verified,0) as proofs_verified,
                   COALESCE(total_bytes_sent,0) as total_bytes_sent,
                   COALESCE(total_bytes_received,0) as total_bytes_received,
                   COALESCE(rate_limit_hits,0) as rate_limit_hits
            FROM usage_metrics
            WHERE period_start >= (now() - ($1 || ' hours')::interval)
            "#,
        )
        .bind(LOOKBACK_HOUR)
        .fetch_all(&**pg)
        .await
        .map_err(VaultlessError::from)?;

        for r in &rows {
            let api_key_id: Uuid = r
                .try_get("api_key_id")
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            let period_start: DateTime<Utc> = r
                .try_get("period_start")
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            let metric_key = MetricKey::new(api_key_id, period_start)?;
            let key_str = metric_key.as_str();

            let mut pipe = redis::pipe();
            pipe.atomic()
                .hset(
                    key_str,
                    "messages_sent",
                    r.try_get::<i64, _>("messages_sent").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "messages_received",
                    r.try_get::<i64, _>("messages_received").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "proofs_verified",
                    r.try_get::<i64, _>("proofs_verified").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "total_bytes_sent",
                    r.try_get::<i64, _>("total_bytes_sent").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "total_bytes_received",
                    r.try_get::<i64, _>("total_bytes_received").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "rate_limit_hits",
                    r.try_get::<i64, _>("rate_limit_hits").unwrap_or(0),
                )
                .sadd(ACTIVE_KEYS_SET.as_str(), key_str)
                .expire(key_str, LOOKBACK_HOUR);
            let _: () = pipe
                .query_async(&mut *conn)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
        }

        tracing::info!("Restored {} usage keys into Redis (fallback)", rows.len());
        return Ok(());
    }

    // 2. Redis has active keys: restore only missing periods
    let mut last_period_per_key: HashMap<Uuid, DateTime<Utc>> = HashMap::new();
    for key_str in active_keys {
        if let Ok(key) = MetricKey::try_from(key_str.to_string()) {
            if let Some((api_key_id, period_start)) = key.parse() {
                last_period_per_key
                    .entry(api_key_id)
                    .and_modify(|existing| {
                        if period_start > *existing {
                            *existing = period_start;
                        }
                    })
                    .or_insert(period_start);
            }
        }
    }
    for (api_key_id, last_period) in last_period_per_key {
        let rows = sqlx::query(
            r#"
            SELECT api_key_id, period_start,
                   COALESCE(messages_sent,0) as messages_sent,
                   COALESCE(messages_received,0) as messages_received,
                   COALESCE(proofs_verified,0) as proofs_verified,
                   COALESCE(total_bytes_sent,0) as total_bytes_sent,
                   COALESCE(total_bytes_received,0) as total_bytes_received,
                   COALESCE(rate_limit_hits,0) as rate_limit_hits
            FROM usage_metrics
            WHERE api_key_id = $1
              AND period_start > $2
            "#,
        )
        .bind(api_key_id)
        .bind(last_period)
        .fetch_all(&**pg)
        .await
        .map_err(VaultlessError::from)?;

        for r in rows {
            let period_start: DateTime<Utc> = r
                .try_get("period_start")
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
            let metric_key = MetricKey::new(api_key_id, period_start)?;
            let key_str = metric_key.as_str();

            let mut pipe = redis::pipe();
            pipe.atomic()
                .hset(
                    key_str,
                    "messages_sent",
                    r.try_get::<i64, _>("messages_sent").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "messages_received",
                    r.try_get::<i64, _>("messages_received").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "proofs_verified",
                    r.try_get::<i64, _>("proofs_verified").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "total_bytes_sent",
                    r.try_get::<i64, _>("total_bytes_sent").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "total_bytes_received",
                    r.try_get::<i64, _>("total_bytes_received").unwrap_or(0),
                )
                .hset(
                    key_str,
                    "rate_limit_hits",
                    r.try_get::<i64, _>("rate_limit_hits").unwrap_or(0),
                )
                .sadd(ACTIVE_KEYS_SET.as_str(), key_str)
                .expire(key_str, LOOKBACK_HOUR);
            let _: () = pipe
                .query_async(&mut *conn)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;
        }
    }

    Ok(())
}
