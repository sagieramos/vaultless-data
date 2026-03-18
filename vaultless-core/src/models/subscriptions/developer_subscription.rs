use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeveloperSubscription {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub application_id: Option<Uuid>,
    pub tier: String,
    pub message_quota: i64,
    pub message_retention_seconds: i64,
    pub rate_limit_per_minute: i32,
    pub is_active: bool,
    pub period_start: DateTime<Utc>,
    pub period_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub bandwidth_quota: i64,
    pub bandwidth_rate_limit_bytes: i64,
    pub proof_enabled: bool,
}

impl DeveloperSubscription {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        developer_id: Uuid,
        application_id: Option<Uuid>,
        tier: String,
        message_quota: i64,
        message_retention_seconds: i64,
        rate_limit_per_minute: i32,
        bandwidth_quota: i64,
        bandwidth_rate_limit_bytes: i64,
        proof_enabled: bool,
    ) -> Result<Self> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO developer_subscriptions (
                developer_id, application_id, tier, message_quota, message_retention_seconds,
                rate_limit_per_minute, bandwidth_quota, bandwidth_rate_limit_bytes, proof_enabled
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
        )
        .bind(developer_id)
        .bind(application_id)
        .bind(tier)
        .bind(message_quota)
        .bind(message_retention_seconds)
        .bind(rate_limit_per_minute)
        .bind(bandwidth_quota)
        .bind(bandwidth_rate_limit_bytes)
        .bind(proof_enabled)
        .fetch_one(&mut **tx)
        .await?;

        Ok(subscription)
    }

    /// Create a free-tier subscription for a new application, expiring in 30 days.
    pub async fn create_free<'c>(
        tx: &mut Transaction<'c, Postgres>,
        developer_id: Uuid,
        application_id: Uuid,
    ) -> Result<Self> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO developer_subscriptions (
                developer_id, application_id, tier,
                period_start, period_end
            )
            VALUES ($1, $2, 'free', NOW(), NOW() + INTERVAL '30 days')
            RETURNING *
            "#,
        )
        .bind(developer_id)
        .bind(application_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(subscription)
    }

    pub async fn find_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            "SELECT * FROM developer_subscriptions WHERE developer_id = $1 AND is_active = true LIMIT 1",
        )
        .bind(developer_id)
        .fetch_optional(executor)
        .await?;

        Ok(subscription)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM developer_subscriptions
            WHERE application_id = $1
              AND is_active = true
              AND (period_end IS NULL OR period_end > NOW())
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(subscription)
    }

    pub async fn update_status<'c, E>(
        executor: E,
        subscription_id: Uuid,
        is_active: bool,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE developer_subscriptions
            SET is_active = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(subscription_id)
        .bind(is_active)
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }

    pub async fn update_period<'c, E>(
        executor: E,
        subscription_id: Uuid,
        period_start: DateTime<Utc>,
        period_end: Option<DateTime<Utc>>,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE developer_subscriptions
            SET period_start = $2, period_end = $3, updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(subscription_id)
        .bind(period_start)
        .bind(period_end)
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }
}