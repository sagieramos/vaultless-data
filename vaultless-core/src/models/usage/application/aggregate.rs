//! TimescaleDB continuous aggregate queries for usage metrics.
//!
//! Queries against the `application_usage_metrics_daily` continuous aggregate
//! for dashboard and billing purposes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::Result;

// =============================================================================
// Daily Usage Summary
// =============================================================================

/// Daily usage summary from the application_usage_metrics_daily continuous aggregate
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DailyUsageSummary {
    pub application_id: Uuid,
    pub subscription_id: Uuid,
    pub day: DateTime<Utc>,
    pub total_messages_sent: Option<i64>,
    pub total_messages_received: Option<i64>,
    pub total_proofs_verified: Option<i64>,
    pub total_bytes_stored: Option<i64>,
    pub total_bytes_sent: Option<i64>,
    pub total_bytes_received: Option<i64>,
    pub total_rate_limit_hits: Option<i64>,
}

impl DailyUsageSummary {
    /// Get daily usage for an application over a date range
    pub async fn get_range(
        pool: &PgPool,
        application_id: Uuid,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as::<_, Self>(
            r#"
            SELECT
                application_id,
                subscription_id,
                day,
                total_messages_sent,
                total_messages_received,
                total_proofs_verified,
                total_bytes_stored,
                total_bytes_sent,
                total_bytes_received,
                total_rate_limit_hits
            FROM application_usage_metrics_daily
            WHERE application_id = $1
                AND day >= $2
                AND day < $3
            ORDER BY day DESC
            "#,
        )
        .bind(application_id)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }

    /// Get last N days of usage for an application
    pub async fn get_last_n_days(
        pool: &PgPool,
        application_id: Uuid,
        days: i32,
    ) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as::<_, Self>(
            r#"
            SELECT
                application_id,
                subscription_id,
                day,
                total_messages_sent,
                total_messages_received,
                total_proofs_verified,
                total_bytes_stored,
                total_bytes_sent,
                total_bytes_received,
                total_rate_limit_hits
            FROM application_usage_metrics_daily
            WHERE application_id = $1
                AND day >= NOW() - INTERVAL '1 day' * $2
            ORDER BY day DESC
            "#,
        )
        .bind(application_id)
        .bind(days)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }

    /// Get current month's total usage for an application
    pub async fn get_current_month_total(
        pool: &PgPool,
        application_id: Uuid,
    ) -> Result<MonthlyTotal> {
        let total = sqlx::query_as::<_, MonthlyTotal>(
            r#"
            SELECT
                $1 as application_id,
                COALESCE(SUM(total_messages_sent)::BIGINT, 0) as total_messages_sent,
                COALESCE(SUM(total_messages_received)::BIGINT, 0) as total_messages_received,
                COALESCE(SUM(total_proofs_verified)::BIGINT, 0) as total_proofs_verified,
                COALESCE(SUM(total_bytes_stored)::BIGINT, 0) as total_bytes_stored,
                COALESCE(SUM(total_bytes_sent)::BIGINT, 0) as total_bytes_sent,
                COALESCE(SUM(total_bytes_received)::BIGINT, 0) as total_bytes_received,
                COALESCE(SUM(total_rate_limit_hits)::BIGINT, 0) as total_rate_limit_hits
            FROM application_usage_metrics_daily
            WHERE application_id = $1
                AND day >= DATE_TRUNC('month', NOW())
                AND day < DATE_TRUNC('month', NOW() + INTERVAL '1 month')
            "#,
        )
        .bind(application_id)
        .fetch_one(pool)
        .await?;

        Ok(total)
    }
}

// =============================================================================
// Monthly Total
// =============================================================================

/// Monthly usage totals
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct MonthlyTotal {
    pub application_id: Uuid,
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_rate_limit_hits: i64,
}

impl Default for MonthlyTotal {
    fn default() -> Self {
        Self {
            application_id: Uuid::nil(),
            total_messages_sent: 0,
            total_messages_received: 0,
            total_proofs_verified: 0,
            total_bytes_stored: 0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            total_rate_limit_hits: 0,
        }
    }
}

// =============================================================================
// Real-time Usage
// =============================================================================

/// Get real-time usage statistics from the raw hypertable
pub async fn get_realtime_usage(
    pool: &PgPool,
    application_id: Uuid,
    since: DateTime<Utc>,
) -> Result<MonthlyTotal> {
    let stats_opt = sqlx::query_as::<_, MonthlyTotal>(
        r#"
        SELECT
            $1 as application_id,
            COALESCE(SUM(messages_sent)::BIGINT, 0) as total_messages_sent,
            COALESCE(SUM(messages_received)::BIGINT, 0) as total_messages_received,
            COALESCE(SUM(proofs_verified)::BIGINT, 0) as total_proofs_verified,
            COALESCE(SUM(total_bytes_stored)::BIGINT, 0) as total_bytes_stored,
            COALESCE(SUM(total_bytes_sent)::BIGINT, 0) as total_bytes_sent,
            COALESCE(SUM(total_bytes_received)::BIGINT, 0) as total_bytes_received,
            COALESCE(SUM(rate_limit_hits)::BIGINT, 0) as total_rate_limit_hits
        FROM application_usage_metrics
        WHERE application_id = $1
            AND period_start >= $2
        "#,
    )
    .bind(application_id)
    .bind(since)
    .fetch_optional(pool)
    .await?;

    Ok(stats_opt.unwrap_or(MonthlyTotal {
        application_id,
        ..Default::default()
    }))
}

// =============================================================================
// Usage Trends
// =============================================================================

/// Usage trend statistics (week over week comparison)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTrends {
    pub current_week: i64,
    pub previous_week: i64,
    pub change_percent: f64,
    /// "up", "down", or "stable"
    pub trend: String,
}

/// Get usage trends for an application (percentage change week over week)
pub async fn get_usage_trends(pool: &PgPool, application_id: Uuid) -> Result<UsageTrends> {
    let row = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
        r#"
        WITH current_week AS (
            SELECT COALESCE(SUM(total_messages_sent)::BIGINT, 0) as total
            FROM application_usage_metrics_daily
            WHERE application_id = $1
                AND day >= DATE_TRUNC('week', NOW())
        ),
        previous_week AS (
            SELECT COALESCE(SUM(total_messages_sent)::BIGINT, 0) as total
            FROM application_usage_metrics_daily
            WHERE application_id = $1
                AND day >= DATE_TRUNC('week', NOW() - INTERVAL '7 days')
                AND day < DATE_TRUNC('week', NOW())
        )
        SELECT
            current_week.total as current,
            previous_week.total as previous
        FROM current_week, previous_week
        "#,
    )
    .bind(application_id)
    .fetch_one(pool)
    .await?;

    let current = row.0.unwrap_or(0);
    let previous = row.1.unwrap_or(0);

    let change_percent = if previous > 0 {
        ((current - previous) as f64 / previous as f64) * 100.0
    } else if current > 0 {
        100.0
    } else {
        0.0
    };

    let trend = if change_percent > 5.0 {
        "up"
    } else if change_percent < -5.0 {
        "down"
    } else {
        "stable"
    };

    Ok(UsageTrends {
        current_week: current,
        previous_week: previous,
        change_percent,
        trend: trend.to_string(),
    })
}
