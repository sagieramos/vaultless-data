//! Redis state restoration from Postgres for client metrics.

use super::config::{RedisPoolType, ACTIVE_CLIENT_KEYS_SET};
use super::counters::{ClientMetricKey, MetricGranularity};
use crate::error::{Result};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

const LOOKBACK_SECS: i64 = 48 * 3600; // 48 hours

/// Restores missing or recent client usage data from Postgres to Redis.
pub async fn restore_client_recent_or_missing_periods_from_pg(
    redis_pool: &Arc<RedisPoolType>,
    pg: Arc<PgPool>,
) -> Result<()> {
    let mut conn = redis_pool.get().await?;

    let active_keys: Vec<String> = redis::cmd("SMEMBERS")
        .arg(ACTIVE_CLIENT_KEYS_SET.as_str())
        .query_async(&mut *conn)
        .await?;

    if active_keys.is_empty() {
        return restore_from_lookback_period(redis_pool, pg).await;
    }

    restore_incrementally(redis_pool, pg, active_keys).await
}

async fn restore_from_lookback_period(
    redis_pool: &Arc<RedisPoolType>,
    pg: Arc<PgPool>,
) -> Result<()> {
    tracing::info!(
        "No active client keys in Redis, restoring last {} hours from Postgres",
        LOOKBACK_SECS / 3600
    );
    let mut conn = redis_pool.get().await?;

    let rows = sqlx::query(
        r#"
        SELECT application_id, client_id, period_start,
               COALESCE(messages_sent, 0) as messages_sent,
               COALESCE(messages_received, 0) as messages_received,
               COALESCE(proofs_verified, 0) as proofs_verified,
               COALESCE(total_bytes_sent, 0) as total_bytes_sent,
               COALESCE(total_bytes_received, 0) as total_bytes_received,
               COALESCE(rate_limit_hits, 0) as rate_limit_hits
        FROM client_usage_metrics
        WHERE period_start >= (now() - ($1 || ' seconds')::interval)
        "#,
    )
    .bind(LOOKBACK_SECS)
    .fetch_all(pg.as_ref())
    .await?;

    for r in &rows {
        let app_id: Uuid = r.try_get("application_id")?;
        let client_id: Uuid = r.try_get("client_id")?;
        let period_start: DateTime<Utc> = r.try_get("period_start")?;

        let metric_key = ClientMetricKey::new(app_id, client_id, period_start, MetricGranularity::Hour)?;
        let key_str = metric_key.as_str();

        let mut pipe = redis::pipe();
        pipe.atomic()
            .hset(key_str, "messages_sent", r.try_get::<i64, _>("messages_sent").unwrap_or(0))
            .hset(key_str, "messages_received", r.try_get::<i64, _>("messages_received").unwrap_or(0))
            .hset(key_str, "proofs_verified", r.try_get::<i64, _>("proofs_verified").unwrap_or(0))
            .hset(key_str, "total_bytes_sent", r.try_get::<i64, _>("total_bytes_sent").unwrap_or(0))
            .hset(key_str, "total_bytes_received", r.try_get::<i64, _>("total_bytes_received").unwrap_or(0))
            .hset(key_str, "rate_limit_hits", r.try_get::<i64, _>("rate_limit_hits").unwrap_or(0))
            .sadd(ACTIVE_CLIENT_KEYS_SET.as_str(), key_str)
            .expire(key_str, LOOKBACK_SECS);
        
        let _: () = pipe.query_async(&mut conn).await?;
    }

    tracing::info!("Restored {} client usage keys into Redis (fallback)", rows.len());
    Ok(())
}

async fn restore_incrementally(
    redis_pool: &Arc<RedisPoolType>,
    pg: Arc<PgPool>,
    active_keys: Vec<String>,
) -> Result<()> {
    let mut last_period_per_client: HashMap<(Uuid, Uuid), DateTime<Utc>> = HashMap::new();
    for key_str in active_keys {
        if let Ok(key) = ClientMetricKey::try_from(key_str) {
            if let Some((app_id, client_id, period_start)) = key.parse() {
                last_period_per_client
                    .entry((app_id, client_id))
                    .and_modify(|e| { *e = (*e).max(period_start); })
                    .or_insert(period_start);
            }
        }
    }

    let mut conn = redis_pool.get().await?;
    for ((app_id, client_id), last_period) in last_period_per_client {
        let rows = sqlx::query(
            r#"
            SELECT period_start,
                   COALESCE(messages_sent, 0) as messages_sent,
                   COALESCE(messages_received, 0) as messages_received,
                   COALESCE(proofs_verified, 0) as proofs_verified,
                   COALESCE(total_bytes_sent, 0) as total_bytes_sent,
                   COALESCE(total_bytes_received, 0) as total_bytes_received,
                   COALESCE(rate_limit_hits, 0) as rate_limit_hits
            FROM client_usage_metrics
            WHERE application_id = $1 AND client_id = $2 AND period_start > $3
            "#,
        )
        .bind(app_id)
        .bind(client_id)
        .bind(last_period)
        .fetch_all(pg.as_ref())
        .await?;

        for r in rows {
            let period_start: DateTime<Utc> = r.try_get("period_start")?;
            let metric_key = ClientMetricKey::new(app_id, client_id, period_start, MetricGranularity::Hour)?;
            let key_str = metric_key.as_str();

            let mut pipe = redis::pipe();
            pipe.atomic()
                .hset(key_str, "messages_sent", r.try_get::<i64, _>("messages_sent").unwrap_or(0))
                .hset(key_str, "messages_received", r.try_get::<i64, _>("messages_received").unwrap_or(0))
                .hset(key_str, "proofs_verified", r.try_get::<i64, _>("proofs_verified").unwrap_or(0))
                .hset(key_str, "total_bytes_sent", r.try_get::<i64, _>("total_bytes_sent").unwrap_or(0))
                .hset(key_str, "total_bytes_received", r.try_get::<i64, _>("total_bytes_received").unwrap_or(0))
                .hset(key_str, "rate_limit_hits", r.try_get::<i64, _>("rate_limit_hits").unwrap_or(0))
                .sadd(ACTIVE_CLIENT_KEYS_SET.as_str(), key_str)
                .expire(key_str, LOOKBACK_SECS);

            let _: () = pipe.query_async(&mut conn).await?;
        }
    }

    Ok(())
}
