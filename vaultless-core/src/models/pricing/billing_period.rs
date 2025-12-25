// vaultless-core/src/models/pricing/billing_period.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::error::Result;

use super::dto::CreateBillingPeriod;
use super::enums::BillingPeriodStatus;

// =============================================================================
// BILLING PERIOD
// =============================================================================

/// Authoritative billing period for an application
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BillingPeriod {
    pub id: Uuid,
    pub application_id: Uuid,
    pub developer_id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub status: BillingPeriodStatus,
    pub created_at: DateTime<Utc>,
}

impl BillingPeriod {
    /// Create a new billing period
    pub async fn create<'c, E>(executor: E, input: CreateBillingPeriod) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO billing_periods (application_id, developer_id, period_start, period_end)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(input.application_id)
        .bind(input.developer_id)
        .bind(input.period_start)
        .bind(input.period_end)
        .fetch_one(executor)
        .await?;

        Ok(period)
    }

    /// Find by ID
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM billing_periods WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| crate::error::VaultlessError::NotFound("Billing period not found".into()))
    }

    /// Get the current open billing period for an application
    pub async fn get_current_open<'c, E>(executor: E, application_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM billing_periods
            WHERE application_id = $1 AND status = 'open'
            ORDER BY period_start DESC
            LIMIT 1
            "#,
        )
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(period)
    }

    /// Get all billing periods for an application
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

    /// Get all billing periods for a developer
    pub async fn find_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let periods = sqlx::query_as::<_, Self>(
            "SELECT * FROM billing_periods WHERE developer_id = $1 ORDER BY period_start DESC",
        )
        .bind(developer_id)
        .fetch_all(executor)
        .await?;

        Ok(periods)
    }

    /// Close a billing period
    pub async fn close<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            r#"
            UPDATE billing_periods SET status = 'closed' WHERE id = $1 RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(executor)
        .await?;

        Ok(period)
    }

    /// Mark billing period as invoiced
    pub async fn mark_invoiced<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let period = sqlx::query_as::<_, Self>(
            r#"
            UPDATE billing_periods SET status = 'invoiced' WHERE id = $1 RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(executor)
        .await?;

        Ok(period)
    }

    /// Check if period has any active subscriptions
    pub async fn has_active_subscriptions<'c, E>(executor: E, id: Uuid) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*) FROM client_subscriptions
            WHERE application_id IN (SELECT application_id FROM billing_periods WHERE id = $1)
            AND status = 'active'
            "#,
        )
        .bind(id)
        .fetch_one(executor)
        .await?;

        Ok(count > 0)
    }
}
