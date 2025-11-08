//vaultless-core/src/models/usage_timescale.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::Result;

/// Daily usage summary (from continuous aggregate)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DailyUsageSummary {
    pub api_key_id: Uuid,
    pub day: DateTime<Utc>,
    pub total_messages_sent: Option<i64>,
    pub total_messages_received: Option<i64>,
    pub total_proofs_verified: Option<i64>,
    pub total_bytes_stored: Option<i64>,
    pub total_bytes_sent: Option<i64>,
    pub total_bytes_received: Option<i64>,
    pub total_rate_limit_hits: Option<i64>,
    pub total_estimated_cost_cents: Option<i64>,
}

/// Weekly usage summary (from continuous aggregate)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct WeeklyUsageSummary {
    pub api_key_id: Uuid,
    pub week_start: DateTime<Utc>,
    pub total_messages_sent: Option<i64>,
    pub total_messages_received: Option<i64>,
    pub total_proofs_verified: Option<i64>,
    pub total_bytes_stored: Option<i64>,
    pub total_bytes_sent: Option<i64>,
    pub total_bytes_received: Option<i64>,
    pub total_rate_limit_hits: Option<i64>,
    pub total_estimated_cost_cents: Option<i64>,
}

impl DailyUsageSummary {
    /// Get daily usage for an API key over a date range
    pub async fn get_range(
        pool: &PgPool,
        api_key_id: Uuid,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as::<_, Self>(
            r#"
            SELECT 
                api_key_id,
                day,
                total_messages_sent,
                total_messages_received,
                total_proofs_verified,
                total_bytes_stored,
                total_bytes_sent,
                total_bytes_received,
                total_rate_limit_hits,
                total_estimated_cost_cents
            FROM usage_metrics_daily
            WHERE api_key_id = $1
                AND day >= $2
                AND day < $3
            ORDER BY day DESC
            "#,
        )
        .bind(api_key_id)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }

    /// Get last N days of usage
    pub async fn get_last_n_days(pool: &PgPool, api_key_id: Uuid, days: i32) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as::<_, Self>(
            r#"
            SELECT 
                api_key_id,
                day,
                total_messages_sent,
                total_messages_received,
                total_proofs_verified,
                total_bytes_stored,
                total_bytes_sent,
                total_bytes_received,
                total_rate_limit_hits,
                total_estimated_cost_cents
            FROM usage_metrics_daily
            WHERE api_key_id = $1
                AND day >= NOW() - INTERVAL '1 day' * $2
            ORDER BY day DESC
            "#,
        )
        .bind(api_key_id)
        .bind(days)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }

    /// Get current month's total usage
    pub async fn get_current_month_total(pool: &PgPool, api_key_id: Uuid) -> Result<MonthlyTotal> {
        let total = sqlx::query_as::<_, MonthlyTotal>(
            r#"
            SELECT 
            $1 as api_key_id,
            COALESCE(SUM(total_messages_sent)::BIGINT, 0) as total_messages_sent,
            COALESCE(SUM(total_messages_received)::BIGINT, 0) as total_messages_received,
            COALESCE(SUM(total_proofs_verified)::BIGINT, 0) as total_proofs_verified,
            COALESCE(SUM(total_bytes_stored)::BIGINT, 0) as total_bytes_stored,
            COALESCE(SUM(total_bytes_sent)::BIGINT, 0) as total_bytes_sent,
            COALESCE(SUM(total_bytes_received)::BIGINT, 0) as total_bytes_received,
            COALESCE(SUM(total_rate_limit_hits)::BIGINT, 0) as total_rate_limit_hits,
            COALESCE(SUM(total_estimated_cost_cents)::BIGINT, 0) as total_estimated_cost_cents
            FROM usage_metrics_daily
            WHERE api_key_id = $1
                AND day >= DATE_TRUNC('month', NOW())
                AND day < DATE_TRUNC('month', NOW() + INTERVAL '1 month')
            "#,
        )
        .bind(api_key_id)
        .fetch_one(pool)
        .await?;

        Ok(total)
    }
}

impl WeeklyUsageSummary {
    /// Get weekly usage for an API key
    pub async fn get_range(
        pool: &PgPool,
        api_key_id: Uuid,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as::<_, Self>(
            r#"
            SELECT 
                api_key_id,
                week_start,
                total_messages_sent,
                total_messages_received,
                total_proofs_verified,
                total_bytes_stored,
                total_bytes_sent,
                total_bytes_received,
                total_rate_limit_hits,
                total_estimated_cost_cents
            FROM usage_metrics_weekly
            WHERE api_key_id = $1
                AND week_start >= $2
                AND week_start < $3
            ORDER BY week_start DESC
            "#,
        )
        .bind(api_key_id)
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }

    /// Get last N weeks of usage
    pub async fn get_last_n_weeks(
        pool: &PgPool,
        api_key_id: Uuid,
        weeks: i32,
    ) -> Result<Vec<Self>> {
        let summaries = sqlx::query_as::<_, Self>(
            r#"
            SELECT 
                api_key_id,
                week_start,
                total_messages_sent,
                total_messages_received,
                total_proofs_verified,
                total_bytes_stored,
                total_bytes_sent,
                total_bytes_received,
                total_rate_limit_hits,
                total_estimated_cost_cents
            FROM usage_metrics_weekly
            WHERE api_key_id = $1
                AND week_start >= NOW() - INTERVAL '7 days' * $2
            ORDER BY week_start DESC
            "#,
        )
        .bind(api_key_id)
        .bind(weeks)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct MonthlyTotal {
    pub api_key_id: Uuid,
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_rate_limit_hits: i64,
    pub total_estimated_cost_cents: i64,
}

/// Get real-time usage statistics (from raw hypertable)
pub async fn get_realtime_usage(
    pool: &PgPool,
    api_key_id: Uuid,
    since: DateTime<Utc>,
) -> Result<MonthlyTotal> {
    let stats_opt = sqlx::query_as::<_, MonthlyTotal>(
        r#"
        SELECT 
            $1 as api_key_id,
            COALESCE(SUM(messages_sent)::BIGINT, 0) as total_messages_sent,
            COALESCE(SUM(messages_received)::BIGINT, 0) as total_messages_received,
            COALESCE(SUM(proofs_verified)::BIGINT, 0) as total_proofs_verified,
            COALESCE(SUM(total_bytes_stored)::BIGINT, 0) as total_bytes_stored,
            COALESCE(SUM(total_bytes_sent)::BIGINT, 0) as total_bytes_sent,
            COALESCE(SUM(total_bytes_received)::BIGINT, 0) as total_bytes_received,
            COALESCE(SUM(rate_limit_hits)::BIGINT, 0) as total_rate_limit_hits,
            COALESCE(SUM(COALESCE(estimated_cost_cents, 0))::BIGINT, 0) as total_estimated_cost_cents

        FROM usage_metrics
        WHERE api_key_id = $1
            AND period_start >= $2
        "#,
    )
    .bind(api_key_id)
    .bind(since)
    .fetch_optional(pool) // CHANGED: Fetch zero or one row
    .await?;

    // If stats_opt is None (meaning no usage data was found at all),
    // return a default MonthlyTotal object with all counts set to 0.
    let stats = stats_opt.unwrap_or(MonthlyTotal {
        api_key_id,
        total_messages_sent: 0,
        total_messages_received: 0,
        total_proofs_verified: 0,
        total_bytes_stored: 0,
        total_bytes_sent: 0,
        total_bytes_received: 0,
        total_rate_limit_hits: 0,
        total_estimated_cost_cents: 0,
    });

    Ok(stats)
}

/// Get usage trends (percentage change)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTrends {
    pub current_week: i64,
    pub previous_week: i64,
    pub change_percent: f64,
    pub trend: String, // "up", "down", "stable"
}

pub async fn get_usage_trends(pool: &PgPool, api_key_id: Uuid) -> Result<UsageTrends> {
    let row = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
        r#"
        WITH current_week AS (
            SELECT COALESCE(SUM(total_messages_sent)::BIGINT, 0) as total
            FROM usage_metrics_daily
            WHERE api_key_id = $1
                AND day >= DATE_TRUNC('week', NOW())
        ),
        previous_week AS (
            SELECT COALESCE(SUM(total_messages_sent)::BIGINT, 0) as total
            FROM usage_metrics_daily
            WHERE api_key_id = $1
                AND day >= DATE_TRUNC('week', NOW() - INTERVAL '7 days')
                AND day < DATE_TRUNC('week', NOW())
        )
        SELECT 
            current_week.total as current,
            previous_week.total as previous
        FROM current_week, previous_week
        "#,
    )
    .bind(api_key_id)
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


