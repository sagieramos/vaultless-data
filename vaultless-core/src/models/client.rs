use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use uuid::Uuid;

/// Represents a cryptographic client credential stored in the database.
/// Standalone record — not bound to a specific user or organization.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Client {
    pub id: Uuid,

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

impl Client {
    /// Create or get client BY HASH.
    pub async fn get_or_create_by_hash<'c, E>(
        executor: E,
        identifier_hash: String,
        public_key: Option<String>,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres> + Copy,
    {
        if let Some(client) = Self::find_by_hash(executor, &identifier_hash).await? {
            return Ok(client);
        }

        let client = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO clients (client_identifier_hash, public_key)
            VALUES ($1, $2)
            RETURNING *
            "#,
        )
        .bind(&identifier_hash)
        .bind(public_key)
        .fetch_one(executor)
        .await?;

        Ok(client)
    }

    /// Find client by its unique hash.
    pub async fn find_by_hash<'c, E>(executor: E, identifier_hash: &str) -> Result<Option<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let client = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM clients
            WHERE client_identifier_hash = $1
            "#,
        )
        .bind(identifier_hash)
        .fetch_optional(executor)
        .await?;

        Ok(client)
    }

    /// Find by internal ID.
    pub async fn find_by_id<'c, E>(executor: E, id: Uuid) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let client = sqlx::query_as::<_, Self>("SELECT * FROM clients WHERE id = $1")
            .bind(id)
            .fetch_optional(executor)
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Client not found".to_string()))?;

        Ok(client)
    }

    /// List all active clients (useful for admin or global lookup).
    pub async fn list_active<'c, E>(executor: E) -> Result<Vec<Self>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let clients = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM clients
            WHERE is_active = true
            ORDER BY last_message_at DESC NULLS LAST
            "#,
        )
        .fetch_all(executor)
        .await?;

        Ok(clients)
    }

    /// Cache key format for verification results
    fn cache_key(identifier_hash: &str, public_key: &str) -> String {
        format!("client:verify:{}:{}", identifier_hash, public_key)
    }

    /// Redis index key to track all verification cache entries for a given identifier
    fn index_key(identifier_hash: &str) -> String {
        format!("client:verify:index:{}", identifier_hash)
    }

    /// Cached credential verification (safe for concurrent use)
    pub async fn verify_client_credentials<'c, E>(
        executor: E,
        redis_pool: &RedisPool,
        identifier_hash: &str,
        public_key: &str,
        ttl_secs: Option<u64>,
    ) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let ttl = ttl_secs.unwrap_or(600);
        let cache_key = Self::cache_key(identifier_hash, public_key);
        let index_key = Self::index_key(identifier_hash);

        let mut redis = redis_pool.get().await?;

        // Attempt to read from cache first
        if let Ok(Some(cached)) = redis.get::<_, Option<String>>(&cache_key).await {
            return Ok(cached == "1");
        }

        // Query the database if not cached
        let exists = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM clients 
                WHERE client_identifier_hash = $1 
                AND public_key = $2
                AND is_active = true
            )
            "#,
        )
        .bind(identifier_hash)
        .bind(public_key)
        .fetch_one(executor)
        .await?;

        // Cache result and register the key in the index set
        let _: () = redis
            .set_ex(&cache_key, if exists { "1" } else { "0" }, ttl)
            .await
            .unwrap_or(());
        let _: () = redis.sadd(&index_key, &cache_key).await.unwrap_or(());

        Ok(exists)
    }

    /// Update a client's public key and invalidate relevant cache entries
    pub async fn update_public_key<'c, E>(
        executor: E,
        redis_pool: &RedisPool,
        identifier_hash: &str,
        new_public_key: String,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres>,
    {
        if new_public_key.is_empty() || new_public_key.len() > 1024 {
            return Err(VaultlessError::Validation(
                "Public key must not be empty or too long".to_string(),
            ));
        }

        let client = sqlx::query_as::<_, Self>(
            r#"
            UPDATE clients
            SET public_key = $1, updated_at = NOW()
            WHERE client_identifier_hash = $2
            RETURNING *
            "#,
        )
        .bind(&new_public_key)
        .bind(identifier_hash)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Client not found".into()))?;

        // Invalidate all cache entries for this client identifier
        let mut redis = redis_pool.get().await?;
        let index_key = Self::index_key(identifier_hash);

        if let Ok(keys) = redis.smembers::<_, Vec<String>>(&index_key).await {
            for key in keys {
                let _: () = redis.del(&key).await.unwrap_or(());
            }
            let _: () = redis.del(&index_key).await.unwrap_or(());
        }

        Ok(client)
    }
}
