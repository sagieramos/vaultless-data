// vaultless-core/src/models/pricing/client_billing_usage.rs

use serde::{Deserialize, Serialize};
use sqlx::{types::Json, Executor, FromRow, Postgres};
use uuid::Uuid;

use crate::error::Result;

use super::snapshot::{PricingSnapshot, RevenueSnapshot};

// =============================================================================
// CLIENT BILLING USAGE
// =============================================================================

/// Frozen usage snapshot for billing
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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
    pub revenue_snapshot: Json<RevenueSnapshot>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl ClientBillingUsage {
    /// Create a new billing usage record
    pub async fn create<'c, E>(
        executor: E,
        billing_period_id: Uuid,
        client_id: Uuid,
        application_id: Uuid,
        developer_id: Uuid,
        messages_sent: i64,
        messages_received: i64,
        proofs_verified: i64,
        total_bytes_stored: i64,
        total_bytes_sent: i64,
        total_bytes_received: i64,
        rate_limit_hits: i32,
        pricing_snapshot: &PricingSnapshot,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Calculate revenue based on usage and pricing
        let revenue = calculate_revenue(
            messages_sent,
            messages_received,
            proofs_verified,
            total_bytes_stored,
            total_bytes_sent,
            total_bytes_received,
            pricing_snapshot,
        );

        let usage = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_billing_usage (
                billing_period_id, client_id, application_id, developer_id,
                messages_sent, messages_received, proofs_verified,
                total_bytes_stored, total_bytes_sent, total_bytes_received,
                rate_limit_hits, revenue_snapshot
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#,
        )
        .bind(billing_period_id)
        .bind(client_id)
        .bind(application_id)
        .bind(developer_id)
        .bind(messages_sent)
        .bind(messages_received)
        .bind(proofs_verified)
        .bind(total_bytes_stored)
        .bind(total_bytes_sent)
        .bind(total_bytes_received)
        .bind(rate_limit_hits)
        .bind(Json(revenue))
        .fetch_one(executor)
        .await?;

        Ok(usage)
    }

    /// Find by ID
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query_as::<_, Self>(
            "SELECT * FROM client_billing_usage WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| crate::error::VaultlessError::NotFound("Billing usage not found".into()))
    }

    /// Get usage for a billing period
    pub async fn find_by_billing_period<'c, E>(executor: E, billing_period_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let usage = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_billing_usage WHERE billing_period_id = $1 ORDER BY created_at",
        )
        .bind(billing_period_id)
        .fetch_all(executor)
        .await?;

        Ok(usage)
    }

    /// Get usage for a client in a billing period
    pub async fn find_by_client_and_period<'c, E>(
        executor: E,
        client_id: Uuid,
        billing_period_id: Uuid,
    ) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let usage = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM client_billing_usage
            WHERE client_id = $1 AND billing_period_id = $2
            "#,
        )
        .bind(client_id)
        .bind(billing_period_id)
        .fetch_optional(executor)
        .await?;

        Ok(usage)
    }

    /// Get total revenue for a billing period (sum of all clients)
    pub async fn get_total_revenue<'c, E>(executor: E, billing_period_id: Uuid) -> Result<RevenueSnapshot>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let result = sqlx::query_as::<_, (
            i64,
            i64,
            i64,
            i64,
            i64,
        )>(
            r#"
            SELECT
                COALESCE(SUM((revenue_snapshot->>'message_revenue_cents')::bigint), 0) as message_revenue,
                COALESCE(SUM((revenue_snapshot->>'storage_revenue_cents')::bigint), 0) as storage_revenue,
                COALESCE(SUM((revenue_snapshot->>'bandwidth_revenue_cents')::bigint), 0) as bandwidth_revenue,
                COALESCE(SUM((revenue_snapshot->>'proof_revenue_cents')::bigint), 0) as proof_revenue,
                COALESCE(SUM((revenue_snapshot->>'total_revenue_cents')::bigint), 0) as total_revenue
            FROM client_billing_usage
            WHERE billing_period_id = $1
            "#,
        )
        .bind(billing_period_id)
        .fetch_one(executor)
        .await?;

        Ok(RevenueSnapshot {
            message_revenue_cents: result.0,
            storage_revenue_cents: result.1,
            bandwidth_revenue_cents: result.2,
            proof_revenue_cents: result.3,
            total_revenue_cents: result.4,
        })
    }
}

/// Calculate revenue based on usage and pricing
fn calculate_revenue(
    messages_sent: i64,
    messages_received: i64,
    proofs_verified: i64,
    _total_bytes_stored: i64,
    total_bytes_sent: i64,
    total_bytes_received: i64,
    pricing: &PricingSnapshot,
) -> RevenueSnapshot {
    // Total messages = sent + received
    let total_messages = messages_sent + messages_received;

    // Calculate message revenue
    let message_revenue_cents = pricing
        .price_per_message_cents
        .map(|rate| total_messages as i64 * rate)
        .unwrap_or(0);

    // Calculate storage revenue (bytes stored / GB)
    let storage_revenue_cents = pricing
        .price_per_gb_cents
        .map(|rate| {
            let gb_stored = _total_bytes_stored / (1024 * 1024 * 1024);
            gb_stored * rate
        })
        .unwrap_or(0);

    // Calculate bandwidth revenue (bytes sent + received / GB)
    let bandwidth_revenue_cents = pricing
        .price_per_gb_cents
        .map(|rate| {
            let total_bytes = total_bytes_sent + total_bytes_received;
            let gb_transferred = total_bytes / (1024 * 1024 * 1024);
            gb_transferred * rate
        })
        .unwrap_or(0);

    // Calculate proof revenue
    let proof_revenue_cents = pricing
        .price_per_proof_cents
        .map(|rate| proofs_verified * rate)
        .unwrap_or(0);

    // Total revenue
    let total_revenue_cents = message_revenue_cents
        + storage_revenue_cents
        + bandwidth_revenue_cents
        + proof_revenue_cents;

    RevenueSnapshot {
        message_revenue_cents,
        storage_revenue_cents,
        bandwidth_revenue_cents,
        proof_revenue_cents,
        total_revenue_cents,
    }
}
