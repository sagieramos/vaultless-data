// vaultless-core/src/models/pricing/client_invoice.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{types::Json, Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::error::Result;

use super::{
    dto::CreateInvoice,
    enums::InvoiceStatus,
    snapshot::PricingSnapshot,
};

// =============================================================================
// CLIENT INVOICE
// =============================================================================

/// Invoice for a client based on billing period usage
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ClientInvoice {
    pub id: Uuid,
    pub billing_period_id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub developer_id: Uuid,
    pub pricing_snapshot: Json<PricingSnapshot>,
    pub subtotal_cents: i64,
    pub total_cents: i64,
    pub status: InvoiceStatus,
    pub created_at: DateTime<Utc>,
}

impl ClientInvoice {
    /// Create a new invoice
    pub async fn create<'c, E>(
        executor: E,
        input: CreateInvoice,
        pricing_snapshot: PricingSnapshot,
        subtotal_cents: i64,
        total_cents: i64,
    ) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoice = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_invoices (
                billing_period_id, client_id, application_id, developer_id,
                pricing_snapshot, subtotal_cents, total_cents
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(input.billing_period_id)
        .bind(input.client_id)
        .bind(input.application_id)
        .bind(input.developer_id)
        .bind(Json(pricing_snapshot))
        .bind(subtotal_cents)
        .bind(total_cents)
        .fetch_one(executor)
        .await?;

        Ok(invoice)
    }

    /// Find by ID
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| crate::error::VaultlessError::NotFound("Invoice not found".into()))
    }

    /// Get invoice for a client in a billing period
    pub async fn find_by_client_and_period<'c, E>(
        executor: E,
        client_id: Uuid,
        billing_period_id: Uuid,
    ) -> Result<Option<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoice = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM client_invoices
            WHERE client_id = $1 AND billing_period_id = $2
            "#,
        )
        .bind(client_id)
        .bind(billing_period_id)
        .fetch_optional(executor)
        .await?;

        Ok(invoice)
    }

    /// Get all invoices for a billing period
    pub async fn find_by_billing_period<'c, E>(executor: E, billing_period_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoices = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE billing_period_id = $1 ORDER BY created_at",
        )
        .bind(billing_period_id)
        .fetch_all(executor)
        .await?;

        Ok(invoices)
    }

    /// Get all invoices for a client
    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
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

    /// Get all invoices for an application
    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
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

    /// Get all invoices for a developer (across all their applications)
    pub async fn find_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<Vec<Self>, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoices = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_invoices WHERE developer_id = $1 ORDER BY created_at DESC",
        )
        .bind(developer_id)
        .fetch_all(executor)
        .await?;

        Ok(invoices)
    }

    /// Update invoice status
    pub async fn update_status<'c, E>(executor: E, id: Uuid, status: InvoiceStatus) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let invoice = sqlx::query_as::<_, Self>(
            "UPDATE client_invoices SET status = $1 WHERE id = $2 RETURNING *",
        )
        .bind(status)
        .bind(id)
        .fetch_one(executor)
        .await?;

        Ok(invoice)
    }

    /// Mark invoice as finalized
    pub async fn finalize<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, InvoiceStatus::Finalized).await
    }

    /// Mark invoice as paid
    pub async fn mark_paid<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, InvoiceStatus::Paid).await
    }

    /// Mark invoice as failed
    pub async fn mark_failed<'c, E>(executor: E, id: Uuid) -> Result<Self, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        Self::update_status(executor, id, InvoiceStatus::Failed).await
    }

    /// Get total revenue for a developer
    pub async fn get_total_revenue_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<i64, crate::error::VaultlessError>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(SUM(total_cents), 0) FROM client_invoices
            WHERE developer_id = $1 AND status = 'paid'
            "#,
        )
        .bind(developer_id)
        .fetch_one(executor)
        .await?;

        Ok(total)
    }
}
