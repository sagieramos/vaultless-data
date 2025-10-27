// vaultless-core/src/models/group/sender_keys.rs
//! Sender Keys Protocol for efficient E2EE in large groups
//!
//! Each sender maintains their own chain key that they rotate with each message.
//! Recipients cache the sender's public key to verify signatures.

use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

use super::types::{CacheTTL, GroupCacheKeys};

// ============================================================================
// Sender Key Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SenderKey {
    pub id: Uuid,
    pub group_id: Uuid,
    pub sender_client_id: Uuid,
    pub recipient_client_id: Uuid,
    pub encrypted_chain_key: String,
    pub key_version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderKeyState {
    pub sender_client_id: Uuid,
    pub group_id: Uuid,
    pub chain_key: String,
    pub signing_key_public: String,
    pub key_version: i32,
    pub message_number: i32,
}

impl SenderKey {
    /// Get sender key for a recipient (with caching)
    pub async fn get_for_recipient(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        sender_id: Uuid,
        recipient_id: Uuid,
    ) -> Result<Self> {
        let cache_key = format!(
            "sender_key:{}:{}:{}",
            group_id, sender_id, recipient_id
        );

        // Try cache first
        let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

        if let Some(json_str) = cached {
            if let Ok(key) = serde_json::from_str::<Self>(&json_str) {
                return Ok(key);
            }
        }

        // Fetch from DB
        let key = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM sender_keys
            WHERE group_id = $1 
                AND sender_client_id = $2 
                AND recipient_client_id = $3
            ORDER BY key_version DESC
            LIMIT 1
            "#,
        )
        .bind(group_id)
        .bind(sender_id)
        .bind(recipient_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Sender key not found for recipient".to_string())
        })?;

        // Cache it
        if let Ok(json_str) = serde_json::to_string(&key) {
            let _: () = redis
                .set_ex(&cache_key, json_str, CacheTTL::SENDER_KEY)
                .await
                .unwrap_or(());
        }

        Ok(key)
    }

    /// Get sender's public signing key (with caching)
    pub async fn get_sender_signing_key(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        sender_id: Uuid,
    ) -> Result<String> {
        let cache_key = GroupCacheKeys::sender_key(&group_id, &sender_id);

        // Try cache first
        let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

        if let Some(signing_key) = cached {
            return Ok(signing_key);
        }

        // Fetch from DB
        let signing_key: Option<String> = sqlx::query_scalar(
            r#"
            SELECT sender_chain_public_key FROM group_members
            WHERE group_id = $1 AND client_address = $2 AND status = 'active'
            "#,
        )
        .bind(group_id)
        .bind(sender_id)
        .fetch_optional(pool)
        .await?
        .flatten();

        let signing_key = signing_key.ok_or_else(|| {
            VaultlessError::NotFound("Sender signing key not found".to_string())
        })?;

        // Cache it
        let _: () = redis
            .set_ex(&cache_key, &signing_key, CacheTTL::SENDER_KEY)
            .await
            .unwrap_or(());

        Ok(signing_key)
    }

    /// Get all sender keys for a recipient in a group (lazy load)
    pub async fn get_all_for_recipient(
        pool: &PgPool,
        group_id: Uuid,
        recipient_id: Uuid,
    ) -> Result<Vec<Self>> {
        let keys = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM sender_keys
            WHERE group_id = $1 AND recipient_client_id = $2
            ORDER BY sender_client_id, key_version DESC
            "#,
        )
        .bind(group_id)
        .bind(recipient_id)
        .fetch_all(pool)
        .await?;

        Ok(keys)
    }

    /// Update sender key version (when sender rotates their chain)
    pub async fn rotate_sender_key(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        sender_id: Uuid,
        new_encrypted_keys: Vec<(Uuid, String)>, // (recipient_id, encrypted_chain_key)
        new_signing_key: String,
        new_version: i32,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;

        // Update sender's signing key
        sqlx::query(
            r#"
            UPDATE group_members
            SET 
                sender_chain_public_key = $3,
                sender_key_version = $4
            WHERE group_id = $1 AND client_address = $2
            "#,
        )
        .bind(group_id)
        .bind(sender_id)
        .bind(&new_signing_key)
        .bind(new_version)
        .execute(&mut *tx)
        .await?;

        // Update encrypted keys for all recipients
        for (recipient_id, encrypted_key) in new_encrypted_keys {
            sqlx::query(
                r#"
                INSERT INTO sender_keys (
                    group_id, sender_client_id, recipient_client_id, 
                    encrypted_chain_key, key_version
                )
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (group_id, sender_client_id, recipient_client_id)
                DO UPDATE SET 
                    encrypted_chain_key = EXCLUDED.encrypted_chain_key,
                    key_version = EXCLUDED.key_version,
                    updated_at = NOW()
                "#,
            )
            .bind(group_id)
            .bind(sender_id)
            .bind(recipient_id)
            .bind(&encrypted_key)
            .bind(new_version)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        // Invalidate cache
        let _: () = redis
            .del(GroupCacheKeys::sender_key(&group_id, &sender_id))
            .await
            .unwrap_or(());

        Ok(())
    }

    /// Delete sender keys when member leaves
    pub async fn delete_for_sender(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        group_id: Uuid,
        sender_id: Uuid,
    ) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM sender_keys
            WHERE group_id = $1 AND sender_client_id = $2
            "#,
        )
        .bind(group_id)
        .bind(sender_id)
        .execute(pool)
        .await?;

        // Invalidate cache
        let _: () = redis
            .del(GroupCacheKeys::sender_key(&group_id, &sender_id))
            .await
            .unwrap_or(());

        Ok(())
    }
}