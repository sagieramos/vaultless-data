//! # API Key Model
//!
//! Optimized for high-frequency lookups with Redis caching (~100µs hits).
//! Includes tier-based defaults, quota checks, and cache invalidation hooks.

use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::sync::Arc;
use tracing::{self, error, info};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};
use crate::types::SubscriptionTier;

// =============================================================================
// Type Aliases & Constants
// =============================================================================

pub type RedisPoolType = RedisPool;

const API_KEY_CACHE_TTL: u64 = 600; // 10 minutes
const LAST_USED_WRITE_TTL: u64 = 300;
const QUOTA_CACHE_TTL_SECONDS: u64 = 31 * 24 * 60 * 60; // ~31 days

const PROJECTION: &str = "id, user_id, key_prefix, tier, monthly_message_quota, message_retention_seconds, description, scopes, is_active, created_at, expires_at, last_used_at, rate_limit_per_minute";

// =============================================================================
// Models
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub key_prefix: String,
    // Note: key_hash is present in the DB but often omitted here for public safety reasons.
    pub tier: SubscriptionTier,
    pub monthly_message_quota: i32,
    pub message_retention_seconds: i32,
    pub description: Option<String>,
    pub scopes: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub rate_limit_per_minute: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedApiKeys {
    pub keys: Vec<ApiKey>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Validate, Deserialize)]
pub struct CreateApiKey {
    pub user_id: Uuid,

    pub key_hash: String,

    pub key_prefix: String,

    pub tier: SubscriptionTier,

    #[validate(length(max = 255))]
    pub description: Option<String>,

    #[validate(length(min = 1))]
    pub scopes: Option<String>,

    pub expires_at: Option<DateTime<Utc>>,
}

// =============================================================================
// Cache Key Generators
// =============================================================================

fn cache_key_by_hash(key_hash: &str) -> String {
    format!("api_key:hash:{}", key_hash)
}

fn cache_key_by_id(id: Uuid) -> String {
    format!("api_key:id:{}", id)
}

pub fn quota_cache_key(api_key_id: Uuid) -> String {
    format!("quota_count:{}:{}", api_key_id, Utc::now().format("%Y-%m"))
}

fn cache_key_last_used_write(id: Uuid) -> String {
    format!("api_key:last_used_write:{}", id)
}

async fn update_last_used(pool: sqlx::PgPool, redis: Option<Arc<RedisPoolType>>, id: Uuid) {
    let month_key = cache_key_last_used_write(id);
    let mut proceed_with_db_write = true;

    // --- 1. Redis Check-and-Set for Rate-Limiting ---
    if let Some(redis_pool) = redis {
        if let Ok(mut conn) = redis_pool.get().await {
            let set_result: std::result::Result<Option<String>, redis::RedisError> =
                redis::cmd("SET")
                    .arg(&month_key)
                    .arg(1)
                    .arg("NX")
                    .arg("EX")
                    .arg(LAST_USED_WRITE_TTL)
                    .query_async(&mut *conn)
                    .await;

            match set_result {
                Ok(Some(_)) => {
                    tracing::debug!(api_key_id = %id, "Performing DB write for last_used_at (rate-limit passed).");
                }
                Ok(None) => {
                    proceed_with_db_write = false;
                }
                Err(e) => {
                    tracing::error!(api_key_id = %id, error = %e, "Redis error during SET NX EX check, proceeding to DB update.");
                }
            }
        } else {
            tracing::warn!(api_key_id = %id, "Failed to get Redis connection for last_used_at update, proceeding to DB update.");
        }
    } else {
        tracing::debug!(api_key_id = %id, "Redis not available, performing unconditional DB write.");
    }

    // --- 2. Database Update (Conditional Execution) ---
    if proceed_with_db_write {
        if let Err(e) = sqlx::query!("UPDATE api_keys SET last_used_at = NOW() WHERE id = $1", id)
            .execute(&pool)
            .await
        {
            tracing::error!(api_key_id = %id, error = %e, "Failed to update last_used_at");
        }
    }
}

// =============================================================================
// Implementation
// =============================================================================

impl ApiKey {
    /// Creates a new API key with tier defaults.
    pub async fn create<'c, E>(executor: E, input: CreateApiKey) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let tier = input.tier;

        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                INSERT INTO api_keys (
                    user_id,
                    key_hash,
                    key_prefix,
                    tier,
                    monthly_message_quota,
                    message_retention_seconds,
                    description,
                    scopes,
                    rate_limit_per_minute,
                    expires_at,
                    is_active
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, true)
                RETURNING {}
                "#,
            PROJECTION
        ))
        .bind(input.user_id)
        .bind(&input.key_hash)
        .bind(&input.key_prefix)
        .bind(tier)
        .bind(tier.default_monthly_quota())
        .bind(tier.default_retention_seconds())
        .bind(&input.description)
        .bind(&input.scopes)
        .bind(tier.default_rate_limit())
        .bind(input.expires_at)
        .fetch_one(executor)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("API key already exists".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        Ok(api_key)
    }

    /// Finds API key by hash with Redis caching (TTL 10min) and rate-limited last_used_at update.
    pub async fn find_by_hash(
        pool: &sqlx::PgPool,
        redis: Option<Arc<RedisPoolType>>,
        key_hash: String,
    ) -> Result<ApiKey> {
        let cache_key = cache_key_by_hash(&key_hash);

        // --- 1. Redis Cache Lookup ---
        if let Some(redis_read) = &redis {
            if let Ok(mut conn) = redis_read.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(api_key) = serde_json::from_str::<ApiKey>(&cached_json) {
                        // Fire-and-forget with pool clone
                        let pool_clone = pool.clone();
                        let redis_clone = redis.clone();
                        let id = api_key.id;

                        tokio::spawn(async move {
                            update_last_used(pool_clone, redis_clone, id).await;
                        });

                        return Ok(api_key);
                    }
                }
            }
        }

        // --- 2. Database Fallback ---
        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
        SELECT {}
        FROM api_keys
        WHERE key_hash = $1
        LIMIT 1
        "#,
            PROJECTION
        ))
        .bind(&key_hash)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".into()))?;

        // Spawn background update
        let pool_clone = pool.clone();
        let redis_clone = redis.clone();
        let id = api_key.id;

        tokio::spawn(async move {
            update_last_used(pool_clone, redis_clone, id).await;
        });

        // --- 3. Cache Write ---
        if let Some(redis) = redis {
            if let Ok(serialized) = serde_json::to_string(&api_key) {
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis.get().await {
                        if let Err(e) = conn
                            .set_ex::<_, _, ()>(&cache_key, serialized, API_KEY_CACHE_TTL)
                            .await
                        {
                            error!(cache_key = %cache_key, error = %e, "Failed to write API key cache.");
                        }
                    }
                });
            }
        }

        Ok(api_key)
    }

    /// Finds API key by hash with Redis caching (TTL 10min).
    /// Synchronous version without background last_used_at update.
    /// Use this with generic executors or transactions where spawning is not needed.
    pub async fn find_by_hash_sync<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        key_hash: String,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = cache_key_by_hash(&key_hash);

        // --- 1. Redis Cache Lookup ---
        if let Some(redis_read) = &redis {
            if let Ok(mut conn) = redis_read.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(api_key) = serde_json::from_str::<ApiKey>(&cached_json) {
                        return Ok(api_key);
                    }
                }
            }
        }

        // --- 2. Database Fallback ---
        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
        SELECT {}
        FROM api_keys
        WHERE key_hash = $1
        LIMIT 1
        "#,
            PROJECTION
        ))
        .bind(&key_hash)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".into()))?;

        // --- 3. Cache Write (fire-and-forget, if Redis available) ---
        if let Some(redis) = redis {
            if let Ok(serialized) = serde_json::to_string(&api_key) {
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis.get().await {
                        if let Err(e) = conn
                            .set_ex::<_, _, ()>(&cache_key, serialized, API_KEY_CACHE_TTL)
                            .await
                        {
                            error!(cache_key = %cache_key, error = %e, "Failed to write API key cache.");
                        }
                    }
                });
            }
        }

        Ok(api_key)
    }
    /// Finds API key by full key (hashed internally) with caching and last_used_at update.
    pub async fn find_by_api_key(
        pool: &sqlx::PgPool,
        redis: Option<Arc<RedisPoolType>>,
        api_key: String,
    ) -> Result<ApiKey> {
        let key_hash = crate::crypto::hash_content(api_key.as_bytes());
        Self::find_by_hash(pool, redis, key_hash).await
    }

    /// Finds API key by ID with caching.
    pub async fn find_by_id<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        // Try cache if Redis is available
        if let Some(redis) = &redis {
            let cache_key = cache_key_by_id(id);

            if let Ok(mut conn) = redis.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(api_key) = serde_json::from_str::<ApiKey>(&cached_json) {
                        return Ok(api_key);
                    }
                }
            }
        }

        // Fetch from DB
        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                SELECT {}
                FROM api_keys WHERE id = $1
                "#,
            PROJECTION
        ))
        .bind(id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".to_string()))?;

        // Cache if Redis available
        if let Some(redis) = redis {
            let cache_key = cache_key_by_id(id);
            if let Ok(serialized) = serde_json::to_string(&api_key) {
                // Simplified clone by moving the Arc into the spawned task
                tokio::spawn(async move {
                    if let Ok(mut conn) = redis.get().await {
                        let _: () = conn
                            .set_ex(&cache_key, serialized, API_KEY_CACHE_TTL)
                            .await
                            .unwrap_or_else(|e| {
                                error!(
                                    cache_key = %cache_key,
                                    error = %e,
                                    "Failed to set API key cache key"
                                );
                            });
                    }
                });
            }
        }
        Ok(api_key)
    }

    /// Lists API keys by owner (paginated with total count).
    pub async fn find_by_owner<'c, E>(
        exec: E,
        user_id: Uuid,
        page: Option<i64>,
        page_size: Option<i64>,
    ) -> Result<PaginatedApiKeys>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(50).clamp(1, 100);
        let offset = (page - 1) * page_size;

        // Get total count
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(exec.clone())
            .await?;

        // Get keys
        let keys = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                SELECT {}
                FROM api_keys 
                WHERE user_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
            PROJECTION
        ))
        .bind(user_id)
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

        let has_more = (offset + page_size) < total;

        Ok(PaginatedApiKeys {
            keys,
            total,
            page,
            page_size,
            has_more,
        })
    }

    /// Lists all API keys (paginated with total count).
    pub async fn list<'c, E>(
        exec: E,
        page: Option<i64>,
        page_size: Option<i64>,
    ) -> Result<PaginatedApiKeys>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let page = page.unwrap_or(1).max(1);
        let page_size = page_size.unwrap_or(50).clamp(1, 100);
        let offset = (page - 1) * page_size;

        // Get total count
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys")
            .fetch_one(exec.clone())
            .await?;

        // Get keys
        let keys = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                SELECT {}
                FROM api_keys 
                ORDER BY created_at DESC 
                LIMIT $1 OFFSET $2
                "#,
            PROJECTION
        ))
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

        let has_more = (offset + page_size) < total;

        Ok(PaginatedApiKeys {
            keys,
            total,
            page,
            page_size,
            has_more,
        })
    }

    /// Validates active/expiry.
    pub async fn validate<'c, E>(&self, exec: E, redis: Option<Arc<RedisPoolType>>) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Check if the key is explicitly deactivated
        if !self.is_active {
            return Err(VaultlessError::ApiKeyInactive);
        }

        // 2. Check for expiry date
        if let Some(expires_at) = self.expires_at {
            if expires_at < Utc::now() {
                return Err(VaultlessError::ApiKeyExpired);
            }
        }

        // 3. Perform the ASYNCHRONOUS Quota Check
        // NOTE: check_quota returns Result<bool>, where 'true' means 'allowed'.
        // If it returns 'false', we should fail the validation.
        let quota = self.monthly_message_quota as i64;
        let is_quota_allowed = Self::check_quota(exec, redis, self.id, quota).await?;

        if !is_quota_allowed {
            return Err(VaultlessError::QuotaExceeded(
                "Monthly message quota exceeded.".to_string(),
            ));
        }

        Ok(())
    }

    /// Updates tier with cache invalidation.
    pub async fn update_tier<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
        new_tier: SubscriptionTier,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. FETCH key_hash BEFORE UPDATE
        let key_hash: String = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id = $1")
            .bind(id)
            .fetch_one(exec.clone())
            .await?;

        // 2. Perform the UPDATE (using the original exec)
        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                UPDATE api_keys 
                SET 
                    tier = $2,
                    monthly_message_quota = $3,
                    message_retention_seconds = $4,
                    rate_limit_per_minute = $5
                WHERE id = $1
                RETURNING {}
                "#,
            PROJECTION
        ))
        .bind(id)
        .bind(new_tier)
        .bind(new_tier.default_monthly_quota())
        .bind(new_tier.default_retention_seconds())
        .bind(new_tier.default_rate_limit())
        .fetch_one(exec)
        .await?;

        // 3. Invalidate cache
        Self::invalidate_cache(redis, id, key_hash).await;

        Ok(api_key)
    }

    /// Deactivate key with cache invalidation.
    pub async fn deactivate<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. FETCH key_hash BEFORE UPDATE
        let key_hash: String = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id = $1")
            .bind(id)
            .fetch_one(exec.clone())
            .await?;

        // 2. Perform the UPDATE (using the original exec)
        sqlx::query("UPDATE api_keys SET is_active = false WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        // 3. Invalidate cache
        Self::invalidate_cache(redis, id, key_hash).await;
        Ok(())
    }

    /// Reactivate key with cache invalidation.
    pub async fn reactivate<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. FETCH key_hash BEFORE UPDATE
        let key_hash: String = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id = $1")
            .bind(id)
            .fetch_one(exec.clone())
            .await?;

        // 2. Perform the UPDATE (using the original exec)
        sqlx::query("UPDATE api_keys SET is_active = true WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        // 3. Invalidate cache
        Self::invalidate_cache(redis, id, key_hash).await;

        Ok(())
    }

    /// Update description with cache invalidation.
    pub async fn update_metadata<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
        description: Option<String>,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. FETCH key_hash BEFORE UPDATE
        let key_hash: String = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id = $1")
            .bind(id)
            .fetch_one(exec.clone())
            .await?;

        // 2. Perform the UPDATE (using the original exec)
        let api_key = sqlx::query_as::<_, ApiKey>(&format!(
            r#"
                UPDATE api_keys 
                SET description = COALESCE($2, description)
                WHERE id = $1
                RETURNING {}
                "#,
            PROJECTION
        ))
        .bind(id)
        .bind(&description)
        .fetch_one(exec)
        .await?;

        // 3. Invalidate cache
        Self::invalidate_cache(redis, id, key_hash).await;

        Ok(api_key)
    }

    /// Hard delete key with cache invalidation.
    pub async fn delete<'c, E>(exec: E, redis: Option<Arc<RedisPoolType>>, id: Uuid) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. FETCH key_hash BEFORE DELETE
        let key_hash: String = sqlx::query_scalar("SELECT key_hash FROM api_keys WHERE id = $1")
            .bind(id)
            .fetch_one(exec.clone())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => {
                    VaultlessError::NotFound("API key not found for deletion".to_string())
                }
                _ => VaultlessError::Database(e),
            })?;

        // 2. Perform the DELETE (using the original exec)
        sqlx::query("DELETE FROM api_keys WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        // 3. Invalidate cache
        Self::invalidate_cache(redis, id, key_hash).await;

        Ok(())
    }

    /// Quota check with caching. Uses provided quota limit (avoids refetch).
    /// Note: Relies on external increment call (e.g., after message creation) for accuracy.
    /// Repopulation from DB on cache miss ensures catch-up during Redis downtime.
    pub async fn check_quota<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        api_key_id: Uuid,
        quota: i64,
    ) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Get the key name
        let month_key = quota_cache_key(api_key_id);

        let mut current_count: Option<i64> = None;

        // 2. Try to get the REAL-TIME count from Redis
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                // Get the count. It will be None if the key doesn't exist.
                current_count = conn
                    .get::<_, Option<i64>>(&month_key)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!(error = %e, "Redis GET failed during quota check.");
                        None
                    });
            }
        }

        // 3. Check the count if found in Redis (fast path)
        if let Some(count) = current_count {
            return Ok(count < quota);
        }

        // 4. FALLBACK: Key not in Redis (e.g., first message of month, or Redis restart)
        // We run the slow query ONCE to re-populate the counter.
        info!(api_key_id = %api_key_id, "Re-populating quota cache from database");
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM messages 
            WHERE api_key_id = $1 
              AND created_at >= date_trunc('month', NOW() AT TIME ZONE 'UTC')
            "#,
        )
        .bind(api_key_id)
        .fetch_one(exec)
        .await?;

        // 5. Update the cache with the correct count and a long TTL
        if let Some(redis_pool) = redis {
            let month_key_clone = month_key.clone();
            tokio::spawn(async move {
                if let Ok(mut conn) = redis_pool.get().await {
                    let _: () = conn
                        .set_ex(&month_key_clone, count, QUOTA_CACHE_TTL_SECONDS)
                        .await
                        .unwrap_or_else(|e| {
                            error!(
                                cache_key = %month_key_clone,
                                error = %e,
                                "Failed to set re-populated quota key"
                            );
                        });
                }
            });
        }

        Ok(count < quota)
    }

    /// Increments the monthly quota usage counter in Redis (approximate).
    /// Call this after successfully creating a message to track usage.
    /// Falls back gracefully; repopulation in check_quota handles downtime.
    pub async fn increment_monthly_usage<'c>(
        redis: Option<Arc<RedisPoolType>>,
        api_key_id: Uuid,
    ) -> Result<()> {
        let month_key = quota_cache_key(api_key_id);

        if let Some(redis_pool) = redis {
            tokio::spawn(async move {
                if let Ok(mut conn) = redis_pool.get().await {
                    // INCR always (sets to 1 if new)
                    let new_count: i64 = conn.incr(&month_key, 1i64).await.unwrap_or_else(|e| {
                        error!(cache_key = %month_key, error = %e, "Failed to increment quota in Redis");
                        0
                    });

                    // Ensure long TTL
                    let _: () = conn
                        .expire(&month_key, QUOTA_CACHE_TTL_SECONDS as i64)
                        .await
                        .unwrap_or_else(|e| {
                            error!(cache_key = %month_key, error = %e, "Failed to set quota TTL");
                        });

                    tracing::debug!(api_key_id = %api_key_id, new_count, "Quota usage incremented");
                }
            });
        } else {
            tracing::warn!(api_key_id = %api_key_id, "Redis unavailable during quota increment; relying on DB repopulation");
        }

        Ok(())
    }

    /// Invalidate all caches for an API key (ID, Hash, and Quota)
    async fn invalidate_cache(redis: Option<Arc<RedisPoolType>>, id: Uuid, key_hash: String) {
        if let Some(redis) = redis {
            tokio::spawn(async move {
                if let Ok(mut conn) = redis.get().await {
                    let cache_key_id = cache_key_by_id(id);
                    let cache_key_hash = cache_key_by_hash(&key_hash);

                    // 1. Invalidate API key cache by ID
                    let _: () = conn.del(&cache_key_id).await.unwrap_or_else(|e| {
                        error!(
                            cache_key = %cache_key_id,
                            error = %e,
                            "Failed to invalidate API key ID cache"
                        );
                    });

                    // 2. Invalidate API key cache by Hash
                    let _: () = conn.del(&cache_key_hash).await.unwrap_or_else(|e| {
                        error!(
                            cache_key_hash = %cache_key_hash,
                            error = %e,
                            "Failed to invalidate API key Hash cache"
                        );
                    });

                    // 3. Invalidate quota cache
                    let quota_key = quota_cache_key(id);
                    let _: () = conn.del(&quota_key).await.unwrap_or_else(|e| {
                        error!(
                            cache_key = %quota_key,
                            error = %e,
                            "Failed to invalidate quota cache"
                        );
                    });
                }
            });
        }
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

    #[test]
    fn test_cache_keys() {
        let id = Uuid::new_v4();
        let hash = "test_hash";

        assert_eq!(cache_key_by_hash(hash), "api_key:hash:test_hash");
        assert_eq!(cache_key_by_id(id), format!("api_key:id:{}", id));
    }
}
