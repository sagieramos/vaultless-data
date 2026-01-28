use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PspPayoutItem {
    pub id: Uuid,
    pub payout_id: Uuid,
    pub revenue_share_id: Uuid,
    pub source_amount: i64,              // Amount in source currency (smallest denomination)
    pub destination_amount: i64,         // Amount in destination currency (after FX)
    pub fx_rate: Option<Decimal>,        // Exchange rate used for this specific conversion
    pub fx_provider: Option<String>,     // Provider of the FX rate (e.g., 'internal', 'openexchangerates')
    pub status: String,                  // pending, processed, failed, reconciled
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PspPayoutItem {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        payout_id: Uuid,
        revenue_share_id: Uuid,
        source_amount: i64,
        destination_amount: i64,
        fx_rate: Option<Decimal>,
        fx_provider: Option<String>,
        status: String,
    ) -> Result<Self> {
        let item = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO psp_payout_items (
                payout_id, revenue_share_id, source_amount, destination_amount,
                fx_rate, fx_provider, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(payout_id)
        .bind(revenue_share_id)
        .bind(source_amount)
        .bind(destination_amount)
        .bind(fx_rate)
        .bind(fx_provider)
        .bind(status)
        .fetch_one(&mut **tx)
        .await?;

        Ok(item)
    }

    pub async fn find_by_payout<'c, E>(executor: E, payout_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let items = sqlx::query_as::<_, Self>(
            "SELECT * FROM psp_payout_items WHERE payout_id = $1 ORDER BY created_at",
        )
        .bind(payout_id)
        .fetch_all(executor)
        .await?;

        Ok(items)
    }

    pub async fn find_by_revenue_share<'c, E>(executor: E, revenue_share_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let items = sqlx::query_as::<_, Self>(
            "SELECT * FROM psp_payout_items WHERE revenue_share_id = $1 ORDER BY created_at",
        )
        .bind(revenue_share_id)
        .fetch_all(executor)
        .await?;

        Ok(items)
    }
}