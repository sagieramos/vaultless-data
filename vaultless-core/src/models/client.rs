use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::ToRedisArgs;
use redis::{AsyncCommands, Script};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, PgPool, Postgres, query_builder::QueryBuilder};
use tracing;
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

#[derive(Clone)]
pub struct LastMessageUpdate {
    pub identifier_hash: String,
    pub last_message_at: DateTime<Utc>,
}

const CACHE_PREFIX_VERIFY: &str = "client:verify";
const CACHE_PREFIX_RESOLVE_ID: &str = "client:resolve_id";
const CACHE_PREFIX_PUBLIC_KEY: &str = "client:public_key";
const CACHE_PREFIX_VERSION: &str = "client:version";

impl Client {
    /// Helper to safely get a cached string value, treating errors as misses.
    async fn try_cache_get_string(
        redis: &mut deadpool_redis::Connection,
        key: impl ToRedisArgs + std::fmt::Debug + std::marker::Send + std::marker::Sync,
    ) -> Option<String> {
        match redis.get::<_, Option<String>>(&key).await {
            Ok(opt) => opt,
            Err(err) => {
                tracing::warn!("Redis get failed for key {:?}: {:?}", key, err);
                None
            }
        }
    }

    /// Helper to set a cache string value, logging errors.
    async fn cache_set_string(
        redis: &mut deadpool_redis::Connection,
        key: impl ToRedisArgs + std::fmt::Debug + std::marker::Send + std::marker::Sync,
        value: &str,
        ttl: u64,
    ) {
        if let Err(err) = redis.set_ex::<_, _, ()>(&key, value, ttl).await {
            tracing::warn!("Redis set_ex failed for key {:?}: {:?}", key, err);
        }
    }

    /// Get current version for a client hash, treating errors as version 0.
    async fn get_current_version(redis: &mut deadpool_redis::Connection, hash: &str) -> u64 {
        let version_key = format!("{CACHE_PREFIX_VERSION}:{hash}");
        match redis.get(&version_key).await {
            Ok(Some(v)) => v,
            Ok(None) => 0,
            Err(err) => {
                tracing::warn!(
                    "Redis get version failed for key {}: {:?}",
                    version_key,
                    err
                );
                0
            }
        }
    }

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
        let mut redis = redis_pool.get().await?;

        let current_version = Self::get_current_version(&mut redis, identifier_hash).await;
        let cache_key =
            format!("{CACHE_PREFIX_VERIFY}:{identifier_hash}:{public_key}:v{current_version}");

        let cached_opt = Self::try_cache_get_string(&mut redis, &cache_key).await;
        if let Some(cached) = cached_opt {
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

        // Cache result
        let cache_val = if exists { "1" } else { "0" };
        Self::cache_set_string(&mut redis, cache_key, cache_val, ttl).await;

        Ok(exists)
    }

    /// Resolve client ID by hash (only active clients), with Redis caching.
    pub async fn resolve_id<'c, E>(
        executor: E,
        redis_pool: &RedisPool,
        identifier_hash: &str,
        ttl_secs: Option<u64>,
    ) -> Result<Uuid>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let ttl = ttl_secs.unwrap_or(600);
        let mut redis = redis_pool.get().await?;

        let current_version = Self::get_current_version(&mut redis, identifier_hash).await;
        let cache_key = format!("{CACHE_PREFIX_RESOLVE_ID}:{identifier_hash}:v{current_version}");

        let cached_opt = Self::try_cache_get_string(&mut redis, &cache_key).await;
        if let Some(cached) = cached_opt {
            if cached == "NOT_FOUND" {
                return Err(VaultlessError::NotFound("Client not found".to_string()));
            }
            match Uuid::parse_str(&cached) {
                Ok(id) => {
                    return Ok(id);
                }
                Err(_) => {
                    // Corrupted cache entry
                    if let Err(err) = redis.del::<_, i64>(&cache_key).await {
                        tracing::warn!(
                            "Redis del failed for corrupted key {}: {:?}",
                            cache_key,
                            err
                        );
                    }
                }
            }
        }

        // Query the database if not cached
        let id_opt: Option<Uuid> = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id FROM clients
            WHERE client_identifier_hash = $1 AND is_active = true
            "#,
        )
        .bind(identifier_hash)
        .fetch_optional(executor)
        .await?;

        if id_opt.is_none() {
            Self::cache_set_string(&mut redis, cache_key, "NOT_FOUND", ttl).await;
            return Err(VaultlessError::NotFound("Client not found".to_string()));
        }

        let id = id_opt.unwrap();
        Self::cache_set_string(&mut redis, cache_key, &id.to_string(), ttl).await;

        Ok(id)
    }

    /// Get public key by hash (only active clients), with Redis caching.
    pub async fn get_public_key<'c, E>(
        executor: E,
        redis_pool: &RedisPool,
        identifier_hash: &str,
        ttl_secs: Option<u64>,
    ) -> Result<Option<String>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let ttl = ttl_secs.unwrap_or(600);
        let mut redis = redis_pool.get().await?;

        let current_version = Self::get_current_version(&mut redis, identifier_hash).await;
        let cache_key = format!("{CACHE_PREFIX_PUBLIC_KEY}:{identifier_hash}:v{current_version}");

        let cached_opt = Self::try_cache_get_string(&mut redis, &cache_key).await;
        if let Some(cached) = cached_opt {
            return match cached.as_str() {
                "NOT_FOUND" => Ok(None),
                "NULL" => Ok(None),
                _ => Ok(Some(cached)),
            };
        }

        // Query the database if not cached
        let row_opt: Option<Option<String>> = sqlx::query_scalar(
            r#"
            SELECT public_key FROM clients
            WHERE client_identifier_hash = $1 AND is_active = true
            "#,
        )
        .bind(identifier_hash)
        .fetch_optional(executor)
        .await?;

        if row_opt.is_none() {
            Self::cache_set_string(&mut redis, cache_key, "NOT_FOUND", ttl).await;
            return Ok(None);
        }

        let pk_opt = row_opt.unwrap();
        let cache_val = match &pk_opt {
            None => "NULL".to_string(),
            Some(s) => s.clone(),
        };
        Self::cache_set_string(&mut redis, cache_key, &cache_val, ttl).await;

        Ok(pk_opt)
    }

    /// Update last message timestamp for an active client by hash.
    /// Caches in Redis and flushes to DB only if 5+ minutes have passed since last DB update.
    pub async fn update_last_message_at_enqueue(
        redis_pool: &RedisPool,
        identifier_hash: &str,
    ) -> Result<()> {
        let mut redis = redis_pool.get().await?;

        let msg_key = format!("client:last_message_at:{}", identifier_hash);
        let pending_set = "client:last_message_pending";

        // Prepare the arguments
        let now = Utc::now().to_rfc3339();

        // 1. Define the Lua script for atomic operation
        let script = Script::new(
            r#"
        -- KEYS[1]: client:last_message_at:hash
        -- KEYS[2]: client:last_message_pending
        -- ARGV[1]: the current timestamp (RFC3339 string)
        -- ARGV[2]: the client hash
        
        -- Execute both commands atomically
        redis.call('SET', KEYS[1], ARGV[1])
        redis.call('SADD', KEYS[2], ARGV[2])
        
        -- Return 1 on success (or whatever is appropriate for success status)
        return 1
    "#,
        );

        // 2. Invoke the script
        // Note: The return type of INVOCATION (e.g., i64 for the 'return 1')
        // must be provided as the generic argument to invoke_async.
        script
            .key(&msg_key) // KEYS[1]
            .key(pending_set) // KEYS[2]
            .arg(&now) // ARGV[1]
            .arg(identifier_hash) // ARGV[2]
            // Expect the return value of the script (1) to be an i64
            .invoke_async::<i64>(&mut redis)
            .await?;

        Ok(())
    }

    /// Background worker: flush all pending last_message_at entries to the DB.
    ///
    /// This function does not loop by itself; call it from a periodic task (every 5 minutes).
    pub async fn flush_pending_last_messages(
        db_pool: &PgPool,
        redis_pool: &RedisPool,
        buffer: &mut Vec<LastMessageUpdate>,
    ) -> Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }

        let start = Utc::now();
        let batch = buffer.drain(..).collect::<Vec<_>>();
        let count = batch.len();

        // --- 1. Batch update Postgres ---
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
        UPDATE clients AS c
        SET last_message_at = v.last_message_at,
            updated_at = NOW()
        FROM (VALUES
    "#,
        );

        qb.push_values(&batch, |mut b, update| {
            b.push_bind(&update.identifier_hash)
                .push_bind(update.last_message_at);
        });

        qb.push(
            ") AS v(identifier_hash, last_message_at)
         WHERE c.client_identifier_hash = v.identifier_hash
         AND c.is_active = TRUE;",
        );

        let rows = qb.build().execute(db_pool).await?.rows_affected();

        // --- 2. Clean Redis cache for these clients ---
        let mut redis = redis_pool.get().await?;
        let mut pipe = redis::pipe();

        for update in &batch {
            pipe.srem("client:last_message_pending", &update.identifier_hash);
        }

        if let Err(e) = pipe.query_async::<()>(&mut *redis).await {
            tracing::warn!("Redis cleanup failed: {:?}", e);
        }

        // --- 3. Log ---
        let elapsed = (Utc::now() - start).num_milliseconds();
        tracing::info!(
            flushed = count,
            updated = rows,
            elapsed_ms = elapsed,
            "Flushed last_message_at updates"
        );

        Ok(())
    }

    /// Get the last message timestamp for a client by hash.
    /// Checks Redis cache first, falls back to DB and caches on miss.
    pub async fn get_last_message_at<'c, E>(
        executor: E,
        redis_pool: &RedisPool,
        identifier_hash: &str,
    ) -> Result<Option<DateTime<Utc>>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let mut redis = redis_pool.get().await?;
        let msg_key = format!("client:last_message_at:{}", identifier_hash);

        // Try cache first
        if let Ok(Some(cached_str)) = redis.get::<_, Option<String>>(&msg_key).await {
            if let Ok(parsed) =
                DateTime::parse_from_rfc3339(cached_str.as_str()).map(|d| d.with_timezone(&Utc))
            {
                return Ok(Some(parsed));
            }
        }

        // Cache miss, query DB
        let opt: Option<DateTime<Utc>> = sqlx::query_scalar(
            r#"
            SELECT last_message_at FROM clients
            WHERE client_identifier_hash = $1 AND is_active = true
            "#,
        )
        .bind(identifier_hash)
        .fetch_optional(executor)
        .await?;

        // Cache the result in Redis
        if let Some(dt) = &opt {
            let _ = redis.set::<_, _, ()>(msg_key, dt.to_rfc3339()).await;
        }

        Ok(opt)
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
            return Err(VaultlessError::InvalidInput(
                "Public key length invalid".into(),
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

        // Invalidate by incrementing the version (no individual key deletions needed)
        let mut redis = redis_pool.get().await?;
        let version_key = format!("{CACHE_PREFIX_VERSION}:{identifier_hash}");

        if let Err(err) = redis.incr::<_, i64, i64>(&version_key, 1i64).await {
            tracing::warn!(
                "Redis incr failed for version key {}: {:?}",
                version_key,
                err
            );
        }

        Ok(client)
    }
}
