use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub recipient_id: String,
    pub ciphertext: String,
    pub nonce: String,
    pub content_type: String,
    pub content_size_bytes: i32,
    pub api_key_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accessed_at: Option<DateTime<Utc>>,
    pub access_count: i32,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateMessage {
    #[validate(length(min = 1, max = 255))]
    pub recipient_id: String,
    
    #[validate(length(min = 1))]
    pub ciphertext: String,
    
    #[validate(length(min = 1, max = 32))]
    pub nonce: String,
    
    pub content_type: Option<String>,
    
    #[validate(range(min = 1))]
    pub content_size_bytes: i32,
    
    pub api_key_id: Uuid,
    
    /// TTL in seconds (optional, will use API key's default retention)
    pub ttl_seconds: Option<i32>,
    
    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageMetadata {
    pub id: Uuid,
    pub recipient_id: String,
    pub content_type: String,
    pub content_size_bytes: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub access_count: i32,
    pub max_access_count: Option<i32>,
}

impl Message {
    /// Create a new message
    pub async fn create(pool: &PgPool, input: CreateMessage) -> Result<Self> {
        input.validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // Get API key to determine retention
        let api_key = crate::models::api_key::ApiKey::find_by_id(pool, input.api_key_id).await?;
        api_key.validate()?;

        // Check quota
        let has_quota = crate::models::api_key::ApiKey::check_quota(pool, input.api_key_id).await?;
        if !has_quota {
            return Err(VaultlessError::QuotaExceeded(
                "Monthly message quota exceeded".to_string(),
            ));
        }

        // Calculate expiration
        let ttl_seconds = input.ttl_seconds.unwrap_or(api_key.message_retention_seconds);
        let expires_at = Utc::now() + Duration::seconds(ttl_seconds as i64);

        let message = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO messages (
                recipient_id, ciphertext, nonce, content_type, 
                content_size_bytes, api_key_id, expires_at, 
                max_access_count, require_proof_verification
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
            "#,
        )
        .bind(&input.recipient_id)
        .bind(&input.ciphertext)
        .bind(&input.nonce)
        .bind(input.content_type.as_deref().unwrap_or("application/octet-stream"))
        .bind(input.content_size_bytes)
        .bind(input.api_key_id)
        .bind(expires_at)
        .bind(input.max_access_count)
        .bind(input.require_proof_verification)
        .fetch_one(pool)
        .await?;

        Ok(message)
    }

    /// Find message by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let message = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM messages WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Message not found".to_string()))?;

        Ok(message)
    }

    /// Find undelivered messages for a recipient
    pub async fn find_by_recipient(
        pool: &PgPool,
        recipient_id: &str,
        limit: i64,
    ) -> Result<Vec<Self>> {
        let messages = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM messages 
            WHERE recipient_id = $1 
                AND is_delivered = false
                AND expires_at > NOW()
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(recipient_id)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        Ok(messages)
    }

    /// Mark message as accessed
    pub async fn mark_accessed(pool: &PgPool, id: Uuid) -> Result<Self> {
        let message = sqlx::query_as::<_, Self>(
            r#"
            UPDATE messages 
            SET 
                access_count = access_count + 1,
                accessed_at = NOW(),
                is_delivered = CASE 
                    WHEN max_access_count IS NOT NULL AND access_count + 1 >= max_access_count 
                    THEN true 
                    ELSE is_delivered 
                END,
                delivered_at = CASE 
                    WHEN max_access_count IS NOT NULL AND access_count + 1 >= max_access_count 
                    THEN NOW() 
                    ELSE delivered_at 
                END
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(message)
    }

    /// Mark message as delivered
    pub async fn mark_delivered(pool: &PgPool, id: Uuid) -> Result<Self> {
        let message = sqlx::query_as::<_, Self>(
            r#"
            UPDATE messages 
            SET 
                is_delivered = true,
                delivered_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(message)
    }

    /// Check if message is accessible
    pub fn validate_access(&self) -> Result<()> {
        // Check expiration
        if self.expires_at < Utc::now() {
            return Err(VaultlessError::MessageExpired);
        }

        // Check access count
        if let Some(max_count) = self.max_access_count {
            if self.access_count >= max_count {
                return Err(VaultlessError::MessageAccessLimitReached);
            }
        }

        Ok(())
    }

    /// Get message metadata (without ciphertext)
    pub fn metadata(&self) -> MessageMetadata {
        MessageMetadata {
            id: self.id,
            recipient_id: self.recipient_id.clone(),
            content_type: self.content_type.clone(),
            content_size_bytes: self.content_size_bytes,
            created_at: self.created_at,
            expires_at: self.expires_at,
            access_count: self.access_count,
            max_access_count: self.max_access_count,
        }
    }

    /// Delete expired messages (cleanup job)
    pub async fn cleanup_expired(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM messages WHERE expires_at < NOW()
            "#,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Count messages for an API key in current month
    pub async fn count_monthly(pool: &PgPool, api_key_id: Uuid) -> Result<i64> {
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

        Ok(count)
    }
}