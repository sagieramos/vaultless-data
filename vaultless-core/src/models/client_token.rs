use chrono::{DateTime, Utc};
use getrandom;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ClientAccessToken {
    pub id: Uuid,
    pub client_id: Uuid,
    pub token_hash: String,
    pub token_type: String,
    pub scopes: Option<Vec<String>>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i32,
    pub max_uses: Option<i32>,
    pub is_revoked: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl ClientAccessToken {
    /// Generate new access token (returns full token + hash)
    pub fn generate() -> Result<(String, String)> {
        let mut random_bytes = [0u8; 32];
        getrandom::fill(&mut random_bytes).map_err(|e| {
            crate::error::VaultlessError::Internal(format!("Key generation failed: {}", e))
        })?;
        let token = format!("vlt_client_{}", hex::encode(random_bytes));

        // Hash for storage
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        Ok((token, token_hash))
    }

    /// Create access token for client
    pub async fn create(
        pool: &PgPool,
        client_id: Uuid,
        expires_in_hours: i64,
    ) -> Result<(String, Self)> {
        let (full_token, token_hash) = Self::generate()?;

        let token_record = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO client_access_tokens (
                client_id, token_hash, expires_at
            )
            VALUES ($1, $2, NOW() + ($3 || ' hours')::INTERVAL)
            RETURNING *
            "#,
        )
        .bind(client_id)
        .bind(&token_hash)
        .bind(expires_in_hours)
        .fetch_one(pool)
        .await?;

        Ok((full_token, token_record))
    }

    /// Validate token and return client_id
    pub async fn validate_and_get_client(pool: &PgPool, token: &str) -> Result<Uuid> {
        // Hash the token
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        // Validate and update usage
        let client_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            UPDATE client_access_tokens
            SET last_used_at = NOW(),
                use_count = use_count + 1
            WHERE token_hash = $1
                AND is_revoked = FALSE
                AND expires_at > NOW()
                AND (max_uses IS NULL OR use_count < max_uses)
            RETURNING client_id
            "#,
        )
        .bind(&token_hash)
        .fetch_optional(pool)
        .await?;

        client_id
            .ok_or_else(|| VaultlessError::Unauthorized("Invalid or expired token".to_string()))
    }

    /// Revoke token
    pub async fn revoke(pool: &PgPool, token: &str) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        sqlx::query(
            r#"
            UPDATE client_access_tokens
            SET is_revoked = TRUE, revoked_at = NOW()
            WHERE token_hash = $1
            "#,
        )
        .bind(&token_hash)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Revoke all tokens for a client
    pub async fn revoke_all_for_client(pool: &PgPool, client_id: Uuid) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE client_access_tokens
            SET is_revoked = TRUE, revoked_at = NOW()
            WHERE client_id = $1 AND is_revoked = FALSE
            "#,
        )
        .bind(client_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// List active tokens for client
    pub async fn list_for_client(pool: &PgPool, client_id: Uuid) -> Result<Vec<Self>> {
        let tokens = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM client_access_tokens
            WHERE client_id = $1 
                AND is_revoked = FALSE
                AND expires_at > NOW()
            ORDER BY created_at DESC
            "#,
        )
        .bind(client_id)
        .fetch_all(pool)
        .await?;

        Ok(tokens)
    }
}
