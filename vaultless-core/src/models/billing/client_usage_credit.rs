use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ClientUsageCredit {
    pub id: Uuid,
    pub client_id: Uuid,
    // These are NON-CASH UNITS ONLY - they unlock usage, not real money
    pub credit_balance: i64,
    pub credit_consumed: i64,
    pub credit_provided: i64,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClientUsageCredit {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        client_id: Uuid,
    ) -> Result<Self> {
        let credit = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_usage_credits (client_id)
            VALUES ($1)
            RETURNING *
            "#,
        )
        .bind(client_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(credit)
    }

    pub async fn find_by_client<'c, E>(
        executor: E,
        client_id: Uuid,
    ) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let credit = sqlx::query_as::<_, Self>(
            "SELECT * FROM client_usage_credits WHERE client_id = $1",
        )
        .bind(client_id)
        .fetch_optional(executor)
        .await?;

        Ok(credit)
    }

    pub async fn update_balance<'c, E>(
        tx: &mut Transaction<'c, Postgres>,
        client_id: Uuid,
        credit_change: i64,
    ) -> Result<Self> {
        // First get the current credit record
        let mut credit = Self::find_by_client(&mut **tx, client_id)
            .await?
            .ok_or_else(|| crate::error::VaultlessError::NotFound("Client usage credit not found".into()))?;

        // Calculate new values
        let new_balance = std::cmp::max(0, credit.credit_balance + credit_change);
        let new_consumed = if credit_change < 0 {
            credit.credit_consumed + (-credit_change)
        } else {
            credit.credit_consumed
        };
        let new_provided = if credit_change > 0 {
            credit.credit_provided + credit_change
        } else {
            credit.credit_provided
        };

        // Update the record
        let updated_credit = sqlx::query_as::<_, Self>(
            r#"
            UPDATE client_usage_credits
            SET
                credit_balance = $2,
                credit_consumed = $3,
                credit_provided = $4,
                updated_at = NOW()
            WHERE client_id = $1
            RETURNING *
            "#,
        )
        .bind(client_id)
        .bind(new_balance)
        .bind(new_consumed)
        .bind(new_provided)
        .fetch_one(&mut **tx)
        .await?;

        Ok(updated_credit)
    }
}