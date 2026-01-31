use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClientBillingUsage {
    pub id: Uuid,
    pub billing_period_id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub messages_sent: i64,
    pub messages_received: i64,
    pub proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub rate_limit_hits: i32,
    pub developer_id: Uuid,
    pub revenue_snapshot: serde_json::Value,  // JSONB field storing revenue snapshot
    pub created_at: DateTime<Utc>,
}

impl ClientBillingUsage {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        billing_period_id: Uuid,
        client_id: Uuid,
        application_id: Uuid,
        messages_sent: i64,
        messages_received: i64,
        proofs_verified: i64,
        total_bytes_stored: i64,
        total_bytes_sent: i64,
        total_bytes_received: i64,
        rate_limit_hits: i32,
        developer_id: Uuid,
        revenue_snapshot: serde_json::Value,
    ) -> Result<Self> {
        let usage = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_billing_usage (
                billing_period_id, client_id, application_id, messages_sent, messages_received,
                proofs_verified, total_bytes_stored, total_bytes_sent, total_bytes_received,
                rate_limit_hits, developer_id, revenue_snapshot
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#,
        )
        .bind(billing_period_id)
        .bind(client_id)
        .bind(application_id)
        .bind(messages_sent)
        .bind(messages_received)
        .bind(proofs_verified)
        .bind(total_bytes_stored)
        .bind(total_bytes_sent)
        .bind(total_bytes_received)
        .bind(rate_limit_hits)
        .bind(developer_id)
        .bind(revenue_snapshot)
        .fetch_one(&mut **tx)
        .await?;

        Ok(usage)
    }

    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let usages = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_billing_usage WHERE client_id = $1 ORDER BY created_at DESC",
        )
        .bind(client_id)
        .fetch_all(executor)
        .await?;

        Ok(usages)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let usages = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_billing_usage WHERE application_id = $1 ORDER BY created_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(usages)
    }

    pub async fn find_by_billing_period<'c, E>(executor: E, billing_period_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let usages = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_billing_usage WHERE billing_period_id = $1 ORDER BY created_at DESC",
        )
        .bind(billing_period_id)
        .fetch_all(executor)
        .await?;

        Ok(usages)
    }

    pub async fn find_by_client_and_period<'c, E>(
        executor: E,
        client_id: Uuid,
        billing_period_id: Uuid,
    ) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let usages = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_billing_usage WHERE client_id = $1 AND billing_period_id = $2 ORDER BY created_at DESC",
        )
        .bind(client_id)
        .bind(billing_period_id)
        .fetch_all(executor)
        .await?;

        Ok(usages)
    }
}