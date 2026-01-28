use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PspPayout {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub psp_account_id: Uuid,
    // This is AMOUNT REQUESTED FROM PSP - platform never holds these funds
    pub amount_cents: i64,
    pub currency: String,
    pub psp_payout_request_id: Option<String>,
    pub psp_payout_status: Option<String>,
    pub psp_response_data: Option<serde_json::Value>,
    pub revenue_share_ids: Option<Vec<Uuid>>, // Array of revenue share IDs included
    pub status: String, // pending, processing, sent, delivered, failed, cancelled
    pub requested_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,

    // PSP-agnostic payout contract fields
    pub source_currency: String,        // Settlement currency (e.g., USD)
    pub destination_currency: String,   // Developer's preferred currency
    pub requested_amount: i64,          // Amount requested in source currency
    pub converted_amount: i64,          // Amount after currency conversion
    pub fx_rate: Option<rust_decimal::Decimal>,  // Exchange rate used for conversion
    pub psp_fee_deducted: i64,         // Fee charged by PSP
    pub net_paid_amount: i64,          // Net amount paid to developer after fees
    pub settlement_date: Option<DateTime<Utc>>,  // Expected settlement date from PSP
    pub psp_normalized_response: Option<serde_json::Value>,  // Normalized response from PSP
}

impl PspPayout {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        developer_id: Uuid,
        psp_account_id: Uuid,
        amount_cents: i64,
        currency: String,
        source_currency: String,
        destination_currency: String,
        requested_amount: i64,
        converted_amount: i64,
        fx_rate: Option<rust_decimal::Decimal>,
    ) -> Result<Self> {
        let payout = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO psp_payouts (
                developer_id, psp_account_id, amount_cents, currency,
                source_currency, destination_currency, requested_amount, converted_amount,
                fx_rate, psp_fee_deducted, net_paid_amount
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING *
            "#,
        )
        .bind(developer_id)
        .bind(psp_account_id)
        .bind(amount_cents)
        .bind(currency)
        .bind(source_currency)
        .bind(destination_currency)
        .bind(requested_amount)
        .bind(converted_amount)
        .bind(fx_rate)
        .bind(0) // psp_fee_deducted initially 0
        .bind(converted_amount) // net_paid_amount initially same as converted_amount
        .fetch_one(&mut **tx)
        .await?;

        Ok(payout)
    }

    pub async fn find_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let payouts = sqlx::query_as::<_, Self>(
            "SELECT * FROM psp_payouts WHERE developer_id = $1 ORDER BY requested_at DESC",
        )
        .bind(developer_id)
        .fetch_all(executor)
        .await?;

        Ok(payouts)
    }

    pub async fn update_payout_status<'c, E>(
        executor: E,
        payout_id: Uuid,
        psp_payout_request_id: Option<String>,
        psp_payout_status: Option<String>,
        psp_response_data: Option<serde_json::Value>,
        status: String,
        processed_at: Option<DateTime<Utc>>,
        delivered_at: Option<DateTime<Utc>>,
        failure_reason: Option<String>,
        psp_fee_deducted: Option<i64>,
        net_paid_amount: Option<i64>,
        settlement_date: Option<DateTime<Utc>>,
        psp_normalized_response: Option<serde_json::Value>,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let payout = sqlx::query_as::<_, Self>(
            r#"
            UPDATE psp_payouts
            SET
                psp_payout_request_id = $2,
                psp_payout_status = $3,
                psp_response_data = $4,
                status = $5,
                processed_at = COALESCE($6, processed_at),
                delivered_at = COALESCE($7, delivered_at),
                failure_reason = $8,
                psp_fee_deducted = COALESCE($9, psp_fee_deducted),
                net_paid_amount = COALESCE($10, net_paid_amount),
                settlement_date = COALESCE($11, settlement_date),
                psp_normalized_response = $12,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(payout_id)
        .bind(psp_payout_request_id)
        .bind(psp_payout_status)
        .bind(psp_response_data)
        .bind(status)
        .bind(processed_at)
        .bind(delivered_at)
        .bind(failure_reason)
        .bind(psp_fee_deducted)
        .bind(net_paid_amount)
        .bind(settlement_date)
        .bind(psp_normalized_response)
        .fetch_one(executor)
        .await?;

        Ok(payout)
    }

    pub async fn mark_as_delivered<'c, E>(
        executor: E,
        payout_id: Uuid,
        psp_response_data: Option<serde_json::Value>,
        psp_fee_deducted: Option<i64>,
        net_paid_amount: Option<i64>,
        settlement_date: Option<DateTime<Utc>>,
        psp_normalized_response: Option<serde_json::Value>,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let payout = sqlx::query_as::<_, Self>(
            r#"
            UPDATE psp_payouts
            SET
                status = 'delivered',
                delivered_at = NOW(),
                psp_response_data = COALESCE($2, psp_response_data),
                psp_fee_deducted = COALESCE($3, psp_fee_deducted),
                net_paid_amount = COALESCE($4, net_paid_amount),
                settlement_date = COALESCE($5, settlement_date),
                psp_normalized_response = $6,
                updated_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(payout_id)
        .bind(psp_response_data)
        .bind(psp_fee_deducted)
        .bind(net_paid_amount)
        .bind(settlement_date)
        .bind(psp_normalized_response)
        .fetch_one(executor)
        .await?;

        Ok(payout)
    }

    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let payout = sqlx::query_as::<_, Self>(
            "SELECT * FROM psp_payouts WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| crate::error::VaultlessError::NotFound("PSP payout not found".into()));

        payout
    }
}