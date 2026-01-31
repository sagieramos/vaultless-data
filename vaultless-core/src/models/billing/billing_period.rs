use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct BillingPeriod {
    pub id: Uuid,
    pub application_id: Uuid,
    pub developer_id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub status: String,  // open, closed, invoiced
    pub created_at: DateTime<Utc>,
    pub platform_revenue_cents: i64,
}

impl BillingPeriod {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        application_id: Uuid,
        developer_id: Uuid,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        status: String,
    ) -> Result<Self> {
        let period = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO billing_periods (application_id, developer_id, period_start, period_end, status)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(application_id)
        .bind(developer_id)
        .bind(period_start)
        .bind(period_end)
        .bind(status)
        .fetch_one(&mut **tx)
        .await?;

        Ok(period)
    }

    pub async fn find_current<'c, E>(executor: E, application_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            "SELECT * FROM billing_periods WHERE $1 BETWEEN period_start AND period_end AND application_id = $2 LIMIT 1",
        )
        .bind(Utc::now())
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(period)
    }

    pub async fn find_by_id<'c, E>(executor: E, period_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            "SELECT * FROM billing_periods WHERE id = $1 LIMIT 1",
        )
        .bind(period_id)
        .fetch_optional(executor)
        .await?;

        Ok(period)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let periods = sqlx::query_as::<_, Self>(
            "SELECT * FROM billing_periods WHERE application_id = $1 ORDER BY period_start DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(periods)
    }

    pub async fn find_open_periods<'c, E>(executor: E) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let periods = sqlx::query_as::<_, Self>(
            "SELECT * FROM billing_periods WHERE status = 'open' ORDER BY period_start DESC",
        )
        .fetch_all(executor)
        .await?;

        Ok(periods)
    }

    pub async fn update_status<'c, E>(
        executor: E,
        period_id: Uuid,
        status: String,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            r#"
            UPDATE billing_periods
            SET status = $2
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(period_id)
        .bind(status)
        .fetch_one(executor)
        .await?;

        Ok(period)
    }
}