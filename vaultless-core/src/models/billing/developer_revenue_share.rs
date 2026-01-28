use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeveloperRevenueShare {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub application_id: Uuid,
    pub billing_period_id: Uuid,
    pub messages_processed: i64,
    pub bytes_transferred: i64,
    pub proofs_verified: i64,
    // These are ACCOUNTING METADATA ONLY - not real money held by platform
    pub gross_revenue_cents: i64,
    pub platform_fee_percent: rust_decimal::Decimal,
    pub platform_fee_cents: i64,
    pub net_revenue_cents: i64,
    pub settlement_currency: String, // Settlement currency for this revenue (e.g., USD)
    pub psp_transaction_id: Option<String>,
    pub psp_payout_id: Option<String>,
    pub status: String, // pending_settlement, settled, paid_to_developer, failed
    pub calculated_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
}

impl DeveloperRevenueShare {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        developer_id: Uuid,
        application_id: Uuid,
        billing_period_id: Uuid,
        messages_processed: i64,
        bytes_transferred: i64,
        proofs_verified: i64,
        gross_revenue_cents: i64,
        platform_fee_percent: rust_decimal::Decimal,
        platform_fee_cents: i64,
        net_revenue_cents: i64,
        settlement_currency: String,
    ) -> Result<Self> {
        let share = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO developer_revenue_shares (
                developer_id, application_id, billing_period_id,
                messages_processed, bytes_transferred, proofs_verified,
                gross_revenue_cents, platform_fee_percent, platform_fee_cents, net_revenue_cents,
                settlement_currency
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING *
            "#,
        )
        .bind(developer_id)
        .bind(application_id)
        .bind(billing_period_id)
        .bind(messages_processed)
        .bind(bytes_transferred)
        .bind(proofs_verified)
        .bind(gross_revenue_cents)
        .bind(platform_fee_percent)
        .bind(platform_fee_cents)
        .bind(net_revenue_cents)
        .bind(settlement_currency)
        .fetch_one(&mut **tx)
        .await?;

        Ok(share)
    }

    pub async fn find_by_billing_period<'c, E>(executor: E, billing_period_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let shares = sqlx::query_as::<_, Self>(
            "SELECT * FROM developer_revenue_shares WHERE billing_period_id = $1 ORDER BY calculated_at",
        )
        .bind(billing_period_id)
        .fetch_all(executor)
        .await?;

        Ok(shares)
    }

    pub async fn update_payout_status<'c, E>(
        executor: E,
        share_id: Uuid,
        psp_payout_id: String,
        status: String,
        paid_at: Option<DateTime<Utc>>,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let mut query = String::from(
            r#"
            UPDATE developer_revenue_shares 
            SET psp_payout_id = $2, status = $3,
            "#,
        );

        if paid_at.is_some() {
            query.push_str("paid_at = $4, ");
        } else {
            query.push_str("paid_at = paid_at, "); // Keep existing value
        }

        query.push_str("updated_at = NOW() WHERE id = $1 RETURNING *");

        let share = if let Some(paid_at_val) = paid_at {
            sqlx::query_as::<_, Self>(&query)
                .bind(share_id)
                .bind(psp_payout_id)
                .bind(status)
                .bind(paid_at_val)
                .fetch_one(executor)
                .await?
        } else {
            sqlx::query_as::<_, Self>(&query)
                .bind(share_id)
                .bind(psp_payout_id)
                .bind(status)
                .fetch_one(executor)
                .await?
        };

        Ok(share)
    }
}