use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct ClientInvoice {
    pub id: Uuid,
    pub billing_period_id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub developer_id: Uuid,
    pub pricing_snapshot: serde_json::Value,  // JSONB field storing pricing snapshot
    pub subtotal_cents: i64,
    pub total_cents: i64,
    pub status: String,  // pending, finalized, paid, failed
    pub created_at: DateTime<Utc>,
    pub is_billable_to_client: bool,
    pub converted_to_credits: bool,
}

impl ClientInvoice {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        billing_period_id: Uuid,
        client_id: Uuid,
        application_id: Uuid,
        developer_id: Uuid,
        pricing_snapshot: serde_json::Value,
        subtotal_cents: i64,
        total_cents: i64,
        status: String,
    ) -> Result<Self> {
        let invoice = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_invoices (
                billing_period_id, client_id, application_id, developer_id,
                pricing_snapshot, subtotal_cents, total_cents, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(billing_period_id)
        .bind(client_id)
        .bind(application_id)
        .bind(developer_id)
        .bind(pricing_snapshot)
        .bind(subtotal_cents)
        .bind(total_cents)
        .bind(status)
        .fetch_one(&mut **tx)
        .await?;

        Ok(invoice)
    }

    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoices = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE client_id = $1 ORDER BY created_at DESC",
        )
        .bind(client_id)
        .fetch_all(executor)
        .await?;

        Ok(invoices)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoices = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE application_id = $1 ORDER BY created_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(invoices)
    }

    pub async fn find_by_billing_period<'c, E>(executor: E, billing_period_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoices = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE billing_period_id = $1 ORDER BY created_at DESC",
        )
        .bind(billing_period_id)
        .fetch_all(executor)
        .await?;

        Ok(invoices)
    }

    pub async fn find_by_id<'c, E>(executor: E, invoice_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoice = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE id = $1 LIMIT 1",
        )
        .bind(invoice_id)
        .fetch_optional(executor)
        .await?;

        Ok(invoice)
    }

    pub async fn update_status<'c, E>(
        executor: E,
        invoice_id: Uuid,
        status: String,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoice = sqlx::query_as::<_, Self>(
            r#"
            UPDATE client_invoices
            SET status = $2
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(invoice_id)
        .bind(status)
        .fetch_one(executor)
        .await?;

        Ok(invoice)
    }
}