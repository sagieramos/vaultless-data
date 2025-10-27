// vaultless-core/src/models/group/reactions.rs

use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};

// ============================================================================
// Reaction Models
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MessageReaction {
    pub id: Uuid,
    pub message_id: Uuid,
    pub client_id: Uuid,
    pub encrypted_reaction: String,
    pub nonce: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionSummary {
    pub message_id: Uuid,
    pub reactions: Vec<ReactionCount>,
    pub total_reactions: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionCount {
    pub encrypted_reaction: String,
    pub count: i32,
    pub reacted_by_me: bool,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct AddReactionRequest {
    pub message_id: Uuid,
    pub client_id: Uuid,
    
    #[validate(length(min = 1))]
    pub encrypted_reaction: String,
    
    #[validate(length(min = 1, max = 32))]
    pub nonce: String,
}

impl MessageReaction {
    /// Add reaction to a message (with transaction)
    pub async fn add_reaction(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        input: AddReactionRequest,
    ) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let mut tx = pool.begin().await?;

        // Verify message exists
        let message_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id = $1)"
        )
        .bind(input.message_id)
        .fetch_one(&mut *tx)
        .await?;

        if !message_exists {
            return Err(VaultlessError::NotFound("Message not found".to_string()));
        }

        // Check if client already reacted with same reaction (prevent duplicates)
        let existing: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM message_reactions
            WHERE message_id = $1 
                AND client_id = $2 
                AND encrypted_reaction = $3
            "#,
        )
        .bind(input.message_id)
        .bind(input.client_id)
        .bind(&input.encrypted_reaction)
        .fetch_optional(&mut *tx)
        .await?;

        if existing.is_some() {
            return Err(VaultlessError::Duplicate(
                "Reaction already exists".to_string(),
            ));
        }

        // Add reaction
        let reaction = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO message_reactions (
                message_id, client_id, encrypted_reaction, nonce
            )
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(input.message_id)
        .bind(input.client_id)
        .bind(&input.encrypted_reaction)
        .bind(&input.nonce)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        // Invalidate cache
        Self::invalidate_reaction_cache(redis, input.message_id).await?;

        Ok(reaction)
    }

    /// Remove reaction (with transaction)
    pub async fn remove_reaction(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        message_id: Uuid,
        client_id: Uuid,
        encrypted_reaction: String,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;

        let result = sqlx::query(
            r#"
            DELETE FROM message_reactions
            WHERE message_id = $1 
                AND client_id = $2 
                AND encrypted_reaction = $3
            "#,
        )
        .bind(message_id)
        .bind(client_id)
        .bind(&encrypted_reaction)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(VaultlessError::NotFound("Reaction not found".to_string()));
        }

        tx.commit().await?;

        // Invalidate cache
        Self::invalidate_reaction_cache(redis, message_id).await?;

        Ok(())
    }

    /// Get all reactions for a message (with caching)
    pub async fn get_for_message(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        message_id: Uuid,
    ) -> Result<Vec<Self>> {
        let cache_key = format!("message:{}:reactions", message_id);

        // Try cache first
        let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

        if let Some(json_str) = cached {
            if let Ok(reactions) = serde_json::from_str::<Vec<Self>>(&json_str) {
                return Ok(reactions);
            }
        }

        // Fetch from DB
        let reactions = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM message_reactions
            WHERE message_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(message_id)
        .fetch_all(pool)
        .await?;

        // Cache for 5 minutes
        if let Ok(json_str) = serde_json::to_string(&reactions) {
            let _: () = redis
                .set_ex(&cache_key, json_str, 300)
                .await
                .unwrap_or(());
        }

        Ok(reactions)
    }

    /// Get reaction summary for a message (aggregated)
    pub async fn get_summary(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        message_id: Uuid,
        client_id: Option<Uuid>,
    ) -> Result<ReactionSummary> {
        let reactions = Self::get_for_message(pool, redis, message_id).await?;

        // Group by encrypted_reaction
        let mut reaction_map: std::collections::HashMap<String, (i32, bool)> =
            std::collections::HashMap::new();

        for reaction in reactions {
            let entry = reaction_map
                .entry(reaction.encrypted_reaction.clone())
                .or_insert((0, false));
            entry.0 += 1;

            if Some(reaction.client_id) == client_id {
                entry.1 = true;
            }
        }

        let reactions: Vec<ReactionCount> = reaction_map
            .into_iter()
            .map(|(encrypted_reaction, (count, reacted_by_me))| ReactionCount {
                encrypted_reaction,
                count,
                reacted_by_me,
            })
            .collect();

        let total_reactions = reactions.iter().map(|r| r.count).sum();

        Ok(ReactionSummary {
            message_id,
            reactions,
            total_reactions,
        })
    }

    /// Get reactions by client (all reactions from a specific client)
    pub async fn get_by_client(
        pool: &PgPool,
        client_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Self>> {
        let reactions = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM message_reactions
            WHERE client_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(client_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(pool)
        .await?;

        Ok(reactions)
    }

    /// Delete all reactions for a message (when message is deleted)
    pub async fn delete_for_message(
        pool: &PgPool,
        redis: &mut redis::aio::Connection,
        message_id: Uuid,
    ) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM message_reactions WHERE message_id = $1"
        )
        .bind(message_id)
        .execute(pool)
        .await?;

        // Invalidate cache
        Self::invalidate_reaction_cache(redis, message_id).await?;

        Ok(result.rows_affected())
    }

    /// Invalidate reaction cache
    async fn invalidate_reaction_cache(
        redis: &mut redis::aio::Connection,
        message_id: Uuid,
    ) -> Result<()> {
        let cache_key = format!("message:{}:reactions", message_id);
        let _: () = redis.del(&cache_key).await.unwrap_or(());
        Ok(())
    }
}