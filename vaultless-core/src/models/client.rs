use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Client {
    pub id: Uuid,
    pub user_id: Uuid,

    // ONLY hash stored - NEVER plaintext
    #[serde(skip_serializing)]
    pub client_identifier_hash: String,
    pub public_key: Option<String>,
    pub allow_anonymous_messages: bool,
    pub require_proof_verification: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub metadata: Option<sqlx::types::JsonValue>,
}

/* #[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "text")]
#[serde(rename_all = "lowercase")]
 */
impl Client {
    /// Create or get client BY HASH (client computes hash on their side)
    pub async fn get_or_create_by_hash(
        pool: &PgPool,
        user_id: Uuid,
        identifier_hash: String,
        public_key: Option<String>,
    ) -> Result<Self> {
        // Try to find existing
        if let Some(client) = Self::find_by_hash(pool, user_id, &identifier_hash).await? {
            return Ok(client);
        }

        // Create new
        let client = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO clients (user_id, client_identifier_hash, client_type, public_key)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(&identifier_hash)
        .bind(public_key)
        .fetch_one(pool)
        .await?;

        Ok(client)
    }

    /// Find by hash
    pub async fn find_by_hash(
        pool: &PgPool,
        user_id: Uuid,
        identifier_hash: &str,
    ) -> Result<Option<Self>> {
        let client = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM clients
            WHERE user_id = $1 AND client_identifier_hash = $2
            "#,
        )
        .bind(user_id)
        .bind(identifier_hash)
        .fetch_optional(pool)
        .await?;

        Ok(client)
    }

    /// Find by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let client = sqlx::query_as::<_, Self>("SELECT * FROM clients WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Client not found".to_string()))?;

        Ok(client)
    }

    /// List user's clients (only returns hashes)
    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Self>> {
        let clients = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM clients
            WHERE user_id = $1 AND is_active = true
            ORDER BY last_message_at DESC NULLS LAST
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(clients)
    }
}
