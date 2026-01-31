use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClientSubscription {
    pub id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub pricing_plan_id: Uuid,  // References the pricing plan ID (not application-specific)
    pub status: String,  // active, inactive, cancelled, expired
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,  // For time-limited subscriptions (note: column is ended_at in DB)
    pub pricing_snapshot: serde_json::Value,  // JSONB field storing pricing snapshot
}

impl ClientSubscription {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        client_id: Uuid,
        application_id: Uuid,
        pricing_plan_id: Uuid,
        status: String,
        started_at: DateTime<Utc>,
        ended_at: Option<DateTime<Utc>>,
        pricing_snapshot: serde_json::Value,
    ) -> Result<Self> {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_subscriptions (
                client_id, application_id, pricing_plan_id, status, started_at, ended_at, pricing_snapshot
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(client_id)
        .bind(application_id)
        .bind(pricing_plan_id)
        .bind(status)
        .bind(started_at)
        .bind(ended_at)
        .bind(pricing_snapshot)
        .fetch_one(&mut **tx)
        .await?;

        Ok(subscription)
    }

    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscriptions = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE client_id = $1 AND status = 'active' ORDER BY started_at DESC",
        )
        .bind(client_id)
        .fetch_all(executor)
        .await?;

        Ok(subscriptions)
    }

    pub async fn find_by_client_and_application<'c, E>(
        executor: E,
        client_id: Uuid,
        application_id: Uuid,
    ) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE client_id = $1 AND application_id = $2 AND status = 'active' LIMIT 1",
        )
        .bind(client_id)
        .bind(application_id)
        .fetch_optional(executor)
        .await?;

        Ok(subscription)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscriptions = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_subscriptions WHERE application_id = $1 AND status = 'active' ORDER BY started_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(subscriptions)
    }

    pub async fn update_status<'c, E>(
        executor: E,
        subscription_id: Uuid,
        status: String,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let subscription = sqlx::query_as::<_, Self>(
            r#"
            UPDATE client_subscriptions
            SET status = $2
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(subscription_id)
        .bind(status)
        .fetch_one(executor)
        .await?;

        Ok(subscription)
    }
}