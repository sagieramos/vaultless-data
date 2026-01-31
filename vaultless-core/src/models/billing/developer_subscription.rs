use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeveloperSubscription {
    pub id: Uuid,
    pub developer_id: Uuid,  // References the user who owns applications
    pub tier: String,  // free, paid, enterprise, etc.
    pub monthly_message_quota: i64,
    pub message_retention_seconds: i64,
    pub rate_limit_per_minute: i32,
    pub is_active: bool,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub monthly_bandwidth_quota: i64,
}

impl DeveloperSubscription {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        developer_id: Uuid,
        tier: String,
        monthly_message_quota: i64,
        message_retention_seconds: i64,
        rate_limit_per_minute: i32,
        monthly_bandwidth_quota: i64,
    ) -> Result<Self> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO developer_subscriptions (
                developer_id, tier, monthly_message_quota, message_retention_seconds,
                rate_limit_per_minute, monthly_bandwidth_quota
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(developer_id)
        .bind(tier)
        .bind(monthly_message_quota)
        .bind(message_retention_seconds)
        .bind(rate_limit_per_minute)
        .bind(monthly_bandwidth_quota)
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
        current_period_start: DateTime<Utc>,
        current_period_end: Option<DateTime<Utc>>,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE developer_subscriptions
            SET current_period_start = $2, current_period_end = $3, updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(subscription_id)
        .bind(current_period_start)
        .bind(current_period_end)
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }
}