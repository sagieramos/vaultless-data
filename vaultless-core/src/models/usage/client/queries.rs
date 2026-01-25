//! Postgres queries for client usage metrics aggregation.

use crate::error::{Result, VaultlessError};
use crate::models::usage::config::active_clients_key;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;

// =============================================================================
// Client Usage Aggregate
// =============================================================================

/// Aggregated usage statistics for a client within an application.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, Default)]
pub struct ClientUsageAggregate {
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_rate_limit_hits: i64,
    pub total_estimated_cost_cents: i64,
}

/// Get all-time aggregated usage metrics for a specific client within an application.
pub async fn get_aggregate_by_client_id<'c, E>(
    exec: E,
    application_id: Uuid,
    client_id: Uuid,
) -> Result<ClientUsageAggregate>
where
    E: Executor<'c, Database = Postgres>,
{
    let result = sqlx::query_as::<_, ClientUsageAggregate>(
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
        FROM
            client_usage_metrics
        WHERE
            application_id = $1 AND client_id = $2
        "#,
    )
    .bind(application_id)
    .bind(client_id)
    .fetch_optional(exec)
    .await?;

    Ok(result.unwrap_or_default())
}

// =============================================================================
// Active Clients Count (Redis) - O(1) using SCARD
// =============================================================================

/// Count the number of unique active clients for an application from Redis.
///
/// This is an O(1) operation using Redis SCARD command on the per-application
/// active clients set: `metric:app:{app_id}:active_clients`
///
/// Active clients are added to this set when they send a message (via Lua scripts).
/// The set has a 24-hour TTL that refreshes on activity.
pub async fn get_active_clients_count(
    redis_pool: Arc<deadpool_redis::Pool>,
    application_id: Uuid,
) -> Result<i64> {
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis pool error: {}", e)))?;

    let key = active_clients_key(&application_id);

    // SCARD is O(1) - returns the set cardinality (number of elements)
    let count: i64 = conn
        .scard(&key)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis SCARD error: {}", e)))?;

    Ok(count)
}

/// Get the list of unique active client IDs for an application from Redis.
///
/// Uses SMEMBERS on the per-application active clients set.
/// Note: This is O(N) where N is the number of active clients, but typically
/// this set is bounded and much smaller than scanning all metric keys.
pub async fn get_active_client_ids(
    redis_pool: Arc<deadpool_redis::Pool>,
    application_id: Uuid,
) -> Result<Vec<Uuid>> {
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis pool error: {}", e)))?;

    let key = active_clients_key(&application_id);

    // SMEMBERS returns all members of the set
    let client_ids: Vec<String> = conn
        .smembers(&key)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis SMEMBERS error: {}", e)))?;

    // Parse UUIDs, filtering out any invalid entries
    let uuids: Vec<Uuid> = client_ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();

    Ok(uuids)
}
