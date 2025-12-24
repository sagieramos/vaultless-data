// vaultless-core/src/models/pricing/client_subscription.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{types::Json, Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::error::Result;

use super::{
    dto::CreateClientSubscription,
    enums::SubscriptionStatus,
    snapshot::PricingSnapshot,
};

// =============================================================================
// CLIENT SUBSCRIPTION
// =============================================================================

/// Client's subscription to an application's pricing plan
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ClientSubscription {
    pub id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub pricing_plan_id: Uuid,
    pub status: SubscriptionStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub pricing_snapshot: Json<PricingSnapshot>,
    pub created_at: DateTime<Utc>,
}

impl ClientSubscription {
    /// Check if the subscription is active
    pub fn is_active(&self) -> bool {
        self.status == SubscriptionStatus::Active && self.ended_at.is_none()
    }

    /// Check if the subscription is cancelled
    pub fn is_cancelled(&self) -> bool {
        self.status == SubscriptionStatus::Cancelled
    }

    /// Create a new client subscription
    pub async fn create<'c, E>(executor: E, input: CreateClientSubscription, pricing_snapshot: PricingSnapshot) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Use CTE to atomically cancel old subscriptions and insert new one
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            WITH cancelled AS (
                UPDATE client_subscriptions
                SET status = 'cancelled', ended_at = NOW()
                WHERE client_id = $1 AND application_id = $2 AND status = 'active'
                RETURNING id
            )
            INSERT INTO client_subscriptions (
                client_id, application_id, pricing_plan_id, pricing_snapshot
            )
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(input.client_id)
        .bind(input.application_id)
        .bind(input.pricing_plan_id)
        .bind(Json(pricing_snapshot))
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }

    /// Find subscription by ID
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| crate::error::VaultlessError::NotFound("Subscription not found".into()))
    }

    /// Get active subscription for a client and application
    pub async fn get_active<'c, E>(executor: E, client_id: Uuid, application_id: Uuid) -> Result<Option<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM client_subscriptions
            WHERE client_id = $1 AND application_id = $2 AND status = 'active'
            "#,
        )
        .bind(client_id)
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(subscription)
    }

    /// Get all subscriptions for a client
    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscriptions = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE client_id = $1 ORDER BY started_at DESC",
        )
        .bind(client_id)
        .fetch_all(executor)
        .await?;

        Ok(subscriptions)
    }

    /// Get all subscriptions for an application
    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscriptions = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE application_id = $1 ORDER BY started_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(subscriptions)
    }

    /// Update subscription status
    pub async fn update_status<'c, E>(executor: E, id: Uuid, status: SubscriptionStatus) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE client_subscriptions
            SET status = $1, ended_at = CASE WHEN $1 = 'cancelled' THEN NOW() ELSE ended_at END
            WHERE id = $2
            RETURNING *
            "#,
        )
        .bind(status)
        .bind(id)
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }

    /// Cancel subscription
    pub async fn cancel<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, SubscriptionStatus::Cancelled).await
    }

    /// Pause subscription
    pub async fn pause<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, SubscriptionStatus::Paused).await
    }

    /// Resume subscription
    pub async fn resume<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, SubscriptionStatus::Active).await
    }
}
