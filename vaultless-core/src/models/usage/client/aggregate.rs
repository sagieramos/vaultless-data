//! TimescaleDB continuous aggregate queries for client usage metrics.
//!
//! Queries against the `client_usage_monthly` continuous aggregate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::Result;

// =============================================================================
// Monthly Usage Summary
// =============================================================================

/// Monthly usage summary from the client_usage_monthly continuous aggregate.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MonthlyUsageSummary {
    pub period_start: DateTime<Utc>,
    pub application_id: Uuid,
    pub client_id: Uuid,
    pub messages_sent: Option<i64>,
    pub messages_received: Option<i64>,
    pub proofs_verified: Option<i64>,
    pub total_bytes_stored: Option<i64>,
    pub total_bytes_sent: Option<i64>,
    pub total_bytes_received: Option<i64>,
    pub rate_limit_hits: Option<i64>,
}

impl MonthlyUsageSummary {
    /// Get monthly usage for a client over a date range.
    pub async fn get_range(
        pool: &PgPool,
        application_id: Uuid,
        client_id: Uuid,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Self>> {
        sqlx::query_as(
            r#"
            SELECT
                application_id, client_id, period_start,
                messages_sent, messages_received, proofs_verified,
                total_bytes_stored, total_bytes_sent, total_bytes_received,
                rate_limit_hits
            FROM client_usage_monthly
            WHERE application_id = $1
              AND client_id = $2
              AND period_start >= $3
              AND period_start < $4
            ORDER BY period_start DESC
            "#,
        )
        .bind(application_id)
        .bind(client_id)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Get current month's total usage for a client.
    pub async fn get_current_month_total(
        pool: &PgPool,
        application_id: Uuid,
        client_id: Uuid,
    ) -> Result<ClientMonthlyTotal> {
        sqlx::query_as(
            r#"
            SELECT
                $1 as application_id,
                $2 as client_id,
                COALESCE(SUM(messages_sent), 0) as total_messages_sent,
                COALESCE(SUM(messages_received), 0) as total_messages_received,
                COALESCE(SUM(proofs_verified), 0) as total_proofs_verified,
                COALESCE(SUM(total_bytes_stored), 0) as total_bytes_stored,
                COALESCE(SUM(total_bytes_sent), 0) as total_bytes_sent,
                COALESCE(SUM(total_bytes_received), 0) as total_bytes_received,
                COALESCE(SUM(rate_limit_hits), 0) as total_rate_limit_hits,
            FROM client_usage_monthly
            WHERE application_id = $1
              AND client_id = $2
              AND period_start >= DATE_TRUNC('month', NOW())
              AND period_start < DATE_TRUNC('month', NOW() + INTERVAL '1 month')
            "#,
        )
        .bind(application_id)
        .bind(client_id)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }
}

// =============================================================================
// Monthly Total
// =============================================================================

/// Client monthly usage totals.
#[derive(Debug, Clone, FromRow, Serialize, Default)]
pub struct ClientMonthlyTotal {
    pub application_id: Uuid,
    pub client_id: Uuid,
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_rate_limit_hits: i64,
}
