use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct CreditTransaction {
    pub id: Uuid,
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub transaction_type: String, // credit_purchase, credit_allocation, usage_deduction, refund, expiration
    pub amount: i64, // Amount of credits affected (can be negative)
    pub usage_context: Option<serde_json::Value>, // Details about what the credits were used for
    pub related_transaction_id: Option<Uuid>,
    pub billing_period_id: Option<Uuid>,
    pub status: String, // pending, completed, failed, reversed
    pub created_at: DateTime<Utc>,
}

impl CreditTransaction {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        client_id: Uuid,
        application_id: Uuid,
        transaction_type: String,
        amount: i64,
        usage_context: Option<serde_json::Value>,
        related_transaction_id: Option<Uuid>,
        billing_period_id: Option<Uuid>,
    ) -> Result<Self> {
        let transaction = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO credit_transactions (
                client_id, application_id, transaction_type, amount,
                usage_context, related_transaction_id, billing_period_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(client_id)
        .bind(application_id)
        .bind(transaction_type)
        .bind(amount)
        .bind(usage_context)
        .bind(related_transaction_id)
        .bind(billing_period_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(transaction)
    }

    pub async fn find_by_client<'c, E>(executor: E, client_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let transactions = sqlx::query_as::<_, Self>(
            "SELECT * FROM credit_transactions WHERE client_id = $1 ORDER BY created_at DESC",
        )
        .bind(client_id)
        .fetch_all(executor)
        .await?;

        Ok(transactions)
    }

    pub async fn find_by_application<'c, E>(executor: E, application_id: Uuid) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let transactions = sqlx::query_as::<_, Self>(
            "SELECT * FROM credit_transactions WHERE application_id = $1 ORDER BY created_at DESC",
        )
        .bind(application_id)
        .fetch_all(executor)
        .await?;

        Ok(transactions)
    }
}