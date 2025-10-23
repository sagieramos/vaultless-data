use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Acquire;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate; // Assume InvalidProof variant added: pub enum VaultlessError { ..., InvalidProof(String) }

const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

// Updated: Removed recipient_id
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub ciphertext: String,
    pub nonce: String,
    pub content_type: String,
    pub content_size_bytes: i32,
    #[serde(skip_serializing)]
    pub api_key_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accessed_at: Option<DateTime<Utc>>,
    pub access_count: i32,
    pub is_delivered: bool,
    pub delivered_at: Option<DateTime<Utc>>,
    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool, // Used in mark_accessed/validate_access
    pub sender_client_id: Option<Uuid>,
    pub recipient_client_id: Uuid, // Now required
    pub group_id: Option<Uuid>,
    pub is_group_message: bool,
}

// Updated: Removed recipient_id, require recipient_client_id
#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateMessage {
    pub recipient_client_id: Uuid, // Required now

    #[validate(length(min = 1))]
    pub ciphertext: String,

    #[validate(length(min = 1, max = 32))]
    pub nonce: String,

    pub content_type: Option<String>,

    #[validate(range(min = 1))]
    pub content_size_bytes: i32,

    pub api_key_id: Uuid,

    pub ttl_seconds: Option<i32>,

    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool, // Set via input for sensitive msgs

    pub sender_client_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub is_group_message: bool,
}

// Updated: Removed recipient_id
#[derive(Debug, Clone, Serialize)]
pub struct MessageMetadata {
    pub id: Uuid,
    pub content_type: String,
    pub content_size_bytes: i32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub access_count: i32,
    pub max_access_count: Option<i32>,
    pub require_proof_verification: bool, // Include for UI gating
}

#[derive(Debug, Clone, Serialize)]
pub struct PaginatedMessages {
    pub messages: Vec<Message>,
    pub next_cursor: Option<DateTime<Utc>>,
    pub has_more: bool,
}

impl Message {
    pub async fn create(pool: &PgPool, input: CreateMessage) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        if input.content_size_bytes as usize != input.ciphertext.len() {
            return Err(VaultlessError::Validation("Size mismatch".to_string()));
        }
        if let Some(ttl) = input.ttl_seconds
            && ttl < 0
        {
            return Err(VaultlessError::Validation(
                "TTL must be non-negative".to_string(),
            ));
        }

        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;

        let (message_retention_seconds, quota_limit) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT message_retention_seconds, monthly_message_quota FROM api_keys WHERE id = $1",
        )
        .bind(input.api_key_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(VaultlessError::NotFound("API key not found".to_string()))?;

        let ttl_seconds = input
            .ttl_seconds
            .unwrap_or(message_retention_seconds as i32);
        let expires_at = Utc::now() + Duration::seconds(ttl_seconds as i64);

        let message = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO messages (
                ciphertext, nonce, content_type, 
                content_size_bytes, api_key_id, expires_at, 
                max_access_count, require_proof_verification,
                sender_client_id, recipient_client_id,
                group_id, is_group_message
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#,
        )
        .bind(&input.ciphertext)
        .bind(&input.nonce)
        .bind(
            input
                .content_type
                .as_deref()
                .unwrap_or(DEFAULT_CONTENT_TYPE),
        )
        .bind(input.content_size_bytes)
        .bind(input.api_key_id)
        .bind(expires_at)
        .bind(input.max_access_count)
        .bind(input.require_proof_verification)
        .bind(input.sender_client_id)
        .bind(input.recipient_client_id)
        .bind(input.group_id)
        .bind(input.is_group_message)
        .fetch_one(&mut *tx)
        .await?;

        let rows_affected = sqlx::query(
            r#"
            UPDATE usage_metrics 
            SET message_count = message_count + 1, total_bytes = total_bytes + $3
            WHERE api_key_id = $1 AND period = date_trunc('month', CURRENT_TIMESTAMP)
              AND message_count < $2
            "#,
        )
        .bind(input.api_key_id)
        .bind(quota_limit)
        .bind(input.content_size_bytes as i64)
        .execute(&mut *tx)
        .await?;

        if rows_affected.rows_affected() == 0 {
            let _ = tx.rollback().await;
            return Err(VaultlessError::QuotaExceeded(
                "Monthly message quota exceeded".to_string(),
            ));
        }

        tx.commit().await?;
        Ok(message)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let message = sqlx::query_as::<_, Self>("SELECT * FROM messages WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Message not found".to_string()))?;
        Ok(message)
    }

    // Updated: Now by recipient_client_id
    pub async fn find_by_recipient_client(
        pool: &PgPool,
        recipient_client_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Self>> {
        let limit = limit.clamp(1, 100);
        let messages = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM messages 
            WHERE recipient_client_id = $1 
                AND is_delivered = false
                AND expires_at > NOW()
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(recipient_client_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(VaultlessError::Database)?;
        Ok(messages)
    }

    // Updated pagination methods (client-based, no recipient_id)
    pub async fn find_paginated_by_recipient_client(
        pool: &PgPool,
        recipient_client_id: Uuid,
        limit: i64,
        after: Option<DateTime<Utc>>,
    ) -> Result<PaginatedMessages> {
        let limit = limit.clamp(1, 100);
        let base_sql = r#"
            SELECT * FROM messages 
            WHERE recipient_client_id = $1 
                AND is_delivered = false
                AND expires_at > NOW()
            ORDER BY created_at DESC
            LIMIT $2
        "#;
        let sql = if after.is_some() {
            format!("{} AND created_at < $3", base_sql.trim_end())
        } else {
            base_sql.to_string()
        };
        let mut q = sqlx::query_as::<_, Self>(&sql)
            .bind(recipient_client_id)
            .bind(limit);
        if let Some(cursor) = after {
            q = q.bind(cursor);
        }

        let messages = q.fetch_all(pool).await.map_err(VaultlessError::Database)?;
        let next_cursor = if messages.len() == limit as usize {
            Some(messages.last().unwrap().created_at)
        } else {
            None
        };

        Ok(PaginatedMessages {
            messages,
            next_cursor,
            has_more: next_cursor.is_some(),
        })
    }

    pub async fn find_paginated_by_sender_client(
        pool: &PgPool,
        sender_client_id: Uuid,
        limit: i64,
        after: Option<DateTime<Utc>>,
    ) -> Result<PaginatedMessages> {
        let limit = limit.clamp(1, 100);
        let base_sql = r#"
            SELECT * FROM messages 
            WHERE sender_client_id = $1 
                AND is_delivered = false
                AND expires_at > NOW()
            ORDER BY created_at DESC
            LIMIT $2
        "#;
        let sql = if after.is_some() {
            format!("{} AND created_at < $3", base_sql.trim_end())
        } else {
            base_sql.to_string()
        };
        let mut q = sqlx::query_as::<_, Self>(&sql)
            .bind(sender_client_id)
            .bind(limit);
        if let Some(cursor) = after {
            q = q.bind(cursor);
        }

        let messages = q.fetch_all(pool).await.map_err(VaultlessError::Database)?;
        let next_cursor = if messages.len() == limit as usize {
            Some(messages.last().unwrap().created_at)
        } else {
            None
        };

        Ok(PaginatedMessages {
            messages,
            next_cursor,
            has_more: next_cursor.is_some(),
        })
    }

    pub async fn find_paginated_by_conversation(
        pool: &PgPool,
        client1_id: Uuid, // Either sender or recipient
        client2_id: Uuid, // The other
        limit: i64,
        after: Option<DateTime<Utc>>,
    ) -> Result<PaginatedMessages> {
        let limit = limit.clamp(1, 100);
        let base_sql = r#"
            SELECT * FROM messages 
            WHERE (sender_client_id = $1 AND recipient_client_id = $2)
               OR (sender_client_id = $2 AND recipient_client_id = $1)
                AND is_delivered = false
                AND expires_at > NOW()
            ORDER BY created_at DESC
            LIMIT $3
        "#;
        let sql = if after.is_some() {
            format!("{} AND created_at < $4", base_sql.trim_end())
        } else {
            base_sql.to_string()
        };
        let mut q = sqlx::query_as::<_, Self>(&sql)
            .bind(client1_id)
            .bind(client2_id)
            .bind(limit);
        if let Some(cursor) = after {
            q = q.bind(cursor);
        }

        let messages = q.fetch_all(pool).await.map_err(VaultlessError::Database)?;
        let next_cursor = if messages.len() == limit as usize {
            Some(messages.last().unwrap().created_at)
        } else {
            None
        };

        Ok(PaginatedMessages {
            messages,
            next_cursor,
            has_more: next_cursor.is_some(),
        })
    }

    // Updated: Require proof if flagged
    pub async fn mark_accessed(pool: &PgPool, id: Uuid, proof: Option<&str>) -> Result<Self> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;

        let mut message: Message =
            sqlx::query_as("SELECT * FROM messages WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(VaultlessError::NotFound("Message not found".to_string()))?;

        message.validate_access()?; // Checks expiration/max

        if message.require_proof_verification {
            let provided_proof =
                proof.ok_or(VaultlessError::Validation("Proof required".to_string()))?;
            message.verify_proof(provided_proof)?; // Implement below
        }

        let updated = sqlx::query_as::<_, Self>(
            r#"
            UPDATE messages 
            SET 
                access_count = access_count + 1,
                accessed_at = NOW(),
                is_delivered = CASE 
                    WHEN max_access_count IS NOT NULL AND (access_count + 1) >= max_access_count 
                    THEN true 
                    ELSE is_delivered 
                END,
                delivered_at = CASE 
                    WHEN max_access_count IS NOT NULL AND (access_count + 1) >= max_access_count 
                    THEN NOW() 
                    ELSE delivered_at 
                END
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(updated)
    }

    pub async fn mark_delivered(pool: &PgPool, id: Uuid) -> Result<Self> {
        let mut conn = pool.acquire().await?;
        let mut tx = conn.begin().await?;

        let message = sqlx::query_as::<_, Self>(
            r#"
            UPDATE messages 
            SET is_delivered = true, delivered_at = NOW()
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(message)
    }

    pub fn validate_access(&self) -> Result<()> {
        if self.expires_at < Utc::now() {
            return Err(VaultlessError::MessageExpired);
        }
        if let Some(max_count) = self.max_access_count
            && self.access_count >= max_count
        {
            return Err(VaultlessError::MessageAccessLimitReached);
        }
        // Future: If require_proof_verification, defer to caller proof check
        Ok(())
    }

    // New: Proof verification (stub—implement with your crypto lib)
    pub fn verify_proof(&mut self, provided_proof: &str) -> Result<()> {
        // Example: Verify JWT/signature against nonce + recipient_client_id
        // Use jsonwebtoken::decode or ed25519 verify
        // e.g., if let Ok(claims) = decode::<Claims>(provided_proof, &KEY, &Validation::new(Algorithm::HS256)) {
        //     if claims.nonce == self.nonce && claims.client_id == self.recipient_client_id { Ok(()) } else { Err(...) }
        // } else { Err(VaultlessError::InvalidProof("Invalid signature".to_string())) }
        if provided_proof.is_empty() {
            // Placeholder
            return Err(VaultlessError::InvalidProof);
        }
        Ok(()) // Replace with real verification
    }

    pub fn metadata(&self) -> MessageMetadata {
        MessageMetadata {
            id: self.id,
            content_type: self.content_type.clone(),
            content_size_bytes: self.content_size_bytes,
            created_at: self.created_at,
            expires_at: self.expires_at,
            access_count: self.access_count,
            max_access_count: self.max_access_count,
            require_proof_verification: self.require_proof_verification,
        }
    }

    pub async fn cleanup_expired(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE expires_at < NOW()")
            .execute(pool)
            .await
            .map_err(VaultlessError::Database)?;
        Ok(result.rows_affected())
    }

    pub async fn count_monthly(pool: &PgPool, api_key_id: Uuid) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM messages 
            WHERE api_key_id = $1 
              AND date_trunc('month', created_at) = date_trunc('month', NOW())
            "#,
        )
        .bind(api_key_id)
        .fetch_one(pool)
        .await
        .map_err(VaultlessError::Database)?;
        Ok(count)
    }
}
