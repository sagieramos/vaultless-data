//! Postgres queries for client usage metrics aggregation.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
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
