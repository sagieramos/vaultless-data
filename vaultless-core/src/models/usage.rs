use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct UsageMetric {
    pub id: Uuid,
    pub api_key_id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub messages_sent: i32,
    pub messages_received: i32,
    pub proofs_verified: i32,
    pub total_bytes_stored: i64,
    pub rate_limit_hits: i32,
    pub estimated_cost_cents: Option<i32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct UsageSummary {
    pub api_key_id: Uuid,
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_rate_limit_hits: i64,
    pub total_cost_cents: i64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

impl UsageMetric {
    /// Create or update usage metrics for current hour
    pub async fn record_message_sent(
        pool: &PgPool,
        api_key_id: Uuid,
        size_bytes: i64,
    ) -> Result<()> {
        let now = Utc::now();
        let period_start = now
            .date_naive()
            .and_hms_opt(now.hour(), 0, 0)
            .unwrap()
            .and_utc();
        let period_end = period_start + Duration::hours(1);

        sqlx::query(
            r#"
            INSERT INTO usage_metrics (
                api_key_id, period_start, period_end, 
                messages_sent, total_bytes_stored
            )
            VALUES ($1, $2, $3, 1, $4)
            ON CONFLICT (api_key_id, date_trunc('hour', period_start))
            DO UPDATE SET
                messages_sent = usage_metrics.messages_sent + 1,
                total_bytes_stored = usage_metrics.total_bytes_stored + $4
            "#,
        )
        .bind(api_key_id)
        .bind(period_start)
        .bind(period_end)
        .bind(size_bytes)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Record message received
    pub async fn record_message_received(pool: &PgPool, api_key_id: Uuid) -> Result<()> {
        let now = Utc::now();
        let period_start = now
            .date_naive()
            .and_hms_opt(now.hour(), 0, 0)
            .unwrap()
            .and_utc();
        let period_end = period_start + Duration::hours(1);

        sqlx::query(
            r#"
            INSERT INTO usage_metrics (
                api_key_id, period_start, period_end, messages_received
            )
            VALUES ($1, $2, $3, 1)
            ON CONFLICT (api_key_id, date_trunc('hour', period_start))
            DO UPDATE SET
                messages_received = usage_metrics.messages_received + 1
            "#,
        )
        .bind(api_key_id)
        .bind(period_start)
        .bind(period_end)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Record proof verification
    pub async fn record_proof_verified(pool: &PgPool, api_key_id: Uuid) -> Result<()> {
        let now = Utc::now();
        let period_start = now
            .date_naive()
            .and_hms_opt(now.hour(), 0, 0)
            .unwrap()
            .and_utc();
        let period_end = period_start + Duration::hours(1);

        sqlx::query(
            r#"
            INSERT INTO usage_metrics (
                api_key_id, period_start, period_end, proofs_verified
            )
            VALUES ($1, $2, $3, 1)
            ON CONFLICT (api_key_id, date_trunc('hour', period_start))
            DO UPDATE SET
                proofs_verified = usage_metrics.proofs_verified + 1
            "#,
        )
        .bind(api_key_id)
        .bind(period_start)
        .bind(period_end)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Record rate limit hit
    pub async fn record_rate_limit_hit(pool: &PgPool, api_key_id: Uuid) -> Result<()> {
        let now = Utc::now();
        let period_start = now
            .date_naive()
            .and_hms_opt(now.hour(), 0, 0)
            .unwrap()
            .and_utc();
        let period_end = period_start + Duration::hours(1);

        sqlx::query(
            r#"
            INSERT INTO usage_metrics (
                api_key_id, period_start, period_end, rate_limit_hits
            )
            VALUES ($1, $2, $3, 1)
            ON CONFLICT (api_key_id, date_trunc('hour', period_start))
            DO UPDATE SET
                rate_limit_hits = usage_metrics.rate_limit_hits + 1
            "#,
        )
        .bind(api_key_id)
        .bind(period_start)
        .bind(period_end)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Get usage summary for an API key over a date range
    pub async fn get_summary(
        pool: &PgPool,
        api_key_id: Uuid,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<UsageSummary> {
        let summary = sqlx::query_as::<_, UsageSummary>(
            r#"
            SELECT 
                $1 as api_key_id,
                COALESCE(SUM(messages_sent), 0) as total_messages_sent,
                COALESCE(SUM(messages_received), 0) as total_messages_received,
                COALESCE(SUM(proofs_verified), 0) as total_proofs_verified,
                COALESCE(SUM(total_bytes_stored), 0) as total_bytes_stored,
                COALESCE(SUM(rate_limit_hits), 0) as total_rate_limit_hits,
                COALESCE(SUM(estimated_cost_cents), 0) as total_cost_cents,
                $2 as period_start,
                $3 as period_end
            FROM usage_metrics
            WHERE api_key_id = $1
                AND period_start >= $2
                AND period_end <= $3
            "#,
        )
        .bind(api_key_id)
        .bind(start)
        .bind(end)
        .fetch_one(pool)
        .await?;

        Ok(summary)
    }

    /// Get current month's usage for an API key
    pub async fn get_current_month_usage(pool: &PgPool, api_key_id: Uuid) -> Result<UsageSummary> {
        let now = Utc::now();
        let month_start = now
            .date_naive()
            .with_day(1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();

        Self::get_summary(pool, api_key_id, month_start, now).await
    }

    /// List usage metrics with pagination
    pub async fn list_for_api_key(
        pool: &PgPool,
        api_key_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>> {
        let metrics = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM usage_metrics 
            WHERE api_key_id = $1
            ORDER BY period_start DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(api_key_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(metrics)
    }

    /// Calculate estimated cost based on usage
    pub async fn calculate_cost(
        pool: &PgPool,
        api_key_id: Uuid,
        cost_per_message_cents: f64,
        cost_per_gb_cents: f64,
        cost_per_verification_cents: f64,
    ) -> Result<i32> {
        let summary = Self::get_current_month_usage(pool, api_key_id).await?;

        let message_cost = summary.total_messages_sent as f64 * cost_per_message_cents;
        let storage_cost =
            (summary.total_bytes_stored as f64 / 1_073_741_824.0) * cost_per_gb_cents;
        let verification_cost = summary.total_proofs_verified as f64 * cost_per_verification_cents;

        let total_cost_cents = (message_cost + storage_cost + verification_cost) as i32;

        Ok(total_cost_cents)
    }
}
