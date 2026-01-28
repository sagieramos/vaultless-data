use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PspAccount {
    pub id: Uuid,
    pub developer_id: Uuid,
    pub psp_account_id: String,
    pub psp_customer_id: Option<String>,
    pub account_type: String,
    pub account_details: Option<serde_json::Value>,
    pub is_active: bool,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PspAccount {
    pub async fn create<'c>(
        tx: &mut Transaction<'c, Postgres>,
        developer_id: Uuid,
        psp_account_id: String,
        account_type: String,
        account_details: Option<serde_json::Value>,
    ) -> Result<Self> {
        let account = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO psp_accounts (developer_id, psp_account_id, account_type, account_details)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(developer_id)
        .bind(psp_account_id)
        .bind(account_type)
        .bind(account_details)
        .fetch_one(&mut **tx)
        .await?;

        Ok(account)
    }

    pub async fn find_by_developer<'c, E>(executor: E, developer_id: Uuid) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let account = sqlx::query_as::<_, Self>(
            "SELECT * FROM psp_accounts WHERE developer_id = $1 AND is_active = true",
        )
        .bind(developer_id)
        .fetch_optional(executor)
        .await?;

        Ok(account)
    }

    pub async fn update_verification_status<'c, E>(
        executor: E,
        account_id: Uuid,
        is_verified: bool,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let account = sqlx::query_as::<_, Self>(
            r#"
            UPDATE psp_accounts 
            SET is_verified = $2, updated_at = NOW()
            WHERE id = $1 
            RETURNING *
            "#,
        )
        .bind(account_id)
        .bind(is_verified)
        .fetch_one(executor)
        .await?;

        Ok(account)
    }
}