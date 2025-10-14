use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};
use crate::types::SubscriptionTier;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub key_hash: String,
    pub key_prefix: String,
    pub tier: SubscriptionTier,
    pub monthly_message_quota: i32,
    pub message_retention_seconds: i32,
    pub owner_email: Option<String>,
    pub owner_name: Option<String>,
    pub organization: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub rate_limit_per_minute: i32,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateApiKey {
    #[validate(length(min = 64, max = 64))]
    pub key_hash: String,
    
    #[validate(length(min = 1, max = 8))]
    pub key_prefix: String,
    
    pub tier: SubscriptionTier,
    
    #[validate(email)]
    pub owner_email: Option<String>,
    
    #[validate(length(min = 1, max = 255))]
    pub owner_name: Option<String>,
    
    #[validate(length(min = 1, max = 255))]
    pub organization: Option<String>,
    
    pub expires_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

impl ApiKey {
    /// Create a new API key
    pub async fn create(pool: &PgPool, input: CreateApiKey) -> Result<Self> {
        input.validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let tier = input.tier;
        let api_key = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO api_keys (
                key_hash, key_prefix, tier, monthly_message_quota, 
                message_retention_seconds, owner_email, owner_name, 
                organization, rate_limit_per_minute, expires_at, notes
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING *
            "#,
        )
        .bind(&input.key_hash)
        .bind(&input.key_prefix)
        .bind(tier)
        .bind(tier.default_monthly_quota())
        .bind(tier.default_retention_seconds())
        .bind(&input.owner_email)
        .bind(&input.owner_name)
        .bind(&input.organization)
        .bind(tier.default_rate_limit())
        .bind(input.expires_at)
        .bind(&input.notes)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("API key already exists".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        Ok(api_key)
    }

    /// Find API key by hash
    pub async fn find_by_hash(pool: &PgPool, key_hash: &str) -> Result<Self> {
        let api_key = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM api_keys WHERE key_hash = $1
            "#,
        )
        .bind(key_hash)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".to_string()))?;

        Ok(api_key)
    }

    /// Find API key by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let api_key = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM api_keys WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".to_string()))?;

        Ok(api_key)
    }

    /// List all API keys (with pagination)
    pub async fn list(pool: &PgPool, limit: i64, offset: i64) -> Result<Vec<Self>> {
        let keys = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM api_keys 
            ORDER BY created_at DESC 
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        Ok(keys)
    }

    /// Validate API key and check if it's usable
    pub fn validate(&self) -> Result<()> {
        if !self.is_active {
            return Err(VaultlessError::ApiKeyInactive);
        }

        if let Some(expires_at) = self.expires_at {
            if expires_at < Utc::now() {
                return Err(VaultlessError::ApiKeyExpired);
            }
        }

        Ok(())
    }

    /// Update tier and associated limits
    pub async fn update_tier(
        pool: &PgPool,
        id: Uuid,
        new_tier: SubscriptionTier,
    ) -> Result<Self> {
        let api_key = sqlx::query_as::<_, Self>(
            r#"
            UPDATE api_keys 
            SET 
                tier = $2,
                monthly_message_quota = $3,
                message_retention_seconds = $4,
                rate_limit_per_minute = $5
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(new_tier)
        .bind(new_tier.default_monthly_quota())
        .bind(new_tier.default_retention_seconds())
        .bind(new_tier.default_rate_limit())
        .fetch_one(pool)
        .await?;

        Ok(api_key)
    }

    /// Deactivate API key
    pub async fn deactivate(pool: &PgPool, id: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE api_keys SET is_active = false WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Reactivate API key
    pub async fn reactivate(pool: &PgPool, id: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE api_keys SET is_active = true WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Check if API key has exceeded quota
    pub async fn check_quota(pool: &PgPool, api_key_id: Uuid) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM messages 
            WHERE api_key_id = $1 
                AND created_at > NOW() - INTERVAL '30 days'
            "#,
        )
        .bind(api_key_id)
        .fetch_one(pool)
        .await?;

        let api_key = Self::find_by_id(pool, api_key_id).await?;
        Ok(count < api_key.monthly_message_quota as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_defaults() {
        assert_eq!(SubscriptionTier::Free.default_monthly_quota(), 1_000);
        assert_eq!(SubscriptionTier::Pro.default_rate_limit(), 1_000);
        assert_eq!(SubscriptionTier::Starter.monthly_price_cents(), Some(2_900));
    }
}