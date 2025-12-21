//! Postgres queries for usage metrics aggregation.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

// =============================================================================
// Usage Aggregate
// =============================================================================

/// Aggregated usage statistics for an application
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

impl Default for UsageAggregate {
    fn default() -> Self {
        Self {
            total_messages_sent: 0,
            total_messages_received: 0,
            total_proofs_verified: 0,
            total_bytes_stored: 0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            total_rate_limit_hits: 0,
            total_estimated_cost_cents: 0,
        }
    }
}

/// Get aggregated usage metrics for an application (all-time)
pub async fn get_aggregate_by_application_id<'c, E>(
    exec: E,
    application_id: Uuid,
) -> Result<UsageAggregate>
where
    E: Executor<'c, Database = Postgres>,
{
    let result = sqlx::query_as::<_, UsageAggregate>(
        r#"
        SELECT
            COALESCE(SUM(um.messages_sent), 0) AS total_messages_sent,
            COALESCE(SUM(um.messages_received), 0) AS total_messages_received,
            COALESCE(SUM(um.proofs_verified), 0) AS total_proofs_verified,
            COALESCE(SUM(um.total_bytes_stored), 0) AS total_bytes_stored,
            COALESCE(SUM(um.total_bytes_sent), 0) AS total_bytes_sent,
            COALESCE(SUM(um.total_bytes_received), 0) AS total_bytes_received,
            COALESCE(SUM(um.rate_limit_hits), 0) AS total_rate_limit_hits,
            COALESCE(SUM(um.estimated_cost_cents), 0) AS total_estimated_cost_cents
        FROM
            usage_metrics um
        WHERE
            um.application_id = $1
        "#,
    )
    .bind(application_id)
    .fetch_optional(exec)
    .await?;

    Ok(result.unwrap_or_default())
}
