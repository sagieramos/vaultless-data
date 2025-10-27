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
use tracing::{self, error};
use uuid::Uuid;
use validator::Validate;

use crate::error::{Result, VaultlessError};
use crate::types::SubscriptionTier;

// =============================================================================
// Type Aliases & Constants
// =============================================================================

pub type RedisPoolType = RedisPool;

const API_KEY_CACHE_TTL: u64 = 600; // 10 minutes
const QUOTA_CACHE_TTL: u64 = 3600; // 1 hour

// =============================================================================
// Models
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub user_id: Uuid,
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

fn quota_cache_key(api_key_id: Uuid) -> String {
    format!("quota_count:{}:{}", api_key_id, Utc::now().format("%Y-%m"))
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

        let api_key = sqlx::query_as::<_, ApiKey>(
            r#"
            INSERT INTO api_keys (
                user_id,
                tier,
                monthly_message_quota,
                message_retention_seconds,
                description,
                scopes,
                rate_limit_per_minute,
                expires_at,
                is_active
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, true)
            RETURNING id, user_id, tier, monthly_message_quota,
                      message_retention_seconds, description, scopes,
                      is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            "#,
        )
        .bind(input.user_id)
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

    /// Finds API key by hash with Redis caching (TTL 10min).
    pub async fn find_by_hash<'c, E>(
        exec: E,
        redis: Arc<RedisPoolType>,
        key_hash: &str,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = cache_key_by_hash(key_hash);

        // --- Fast path (Cache Read) ---
        let mut conn = redis
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis pool error: {}", e)))?;

        if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
            if let Ok(api_key) = serde_json::from_str::<ApiKey>(&cached_json) {
                return Ok(api_key);
            }
        }

        // --- Cache miss (Database Read) ---
        let api_key = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT id, user_id, tier, monthly_message_quota,
                   message_retention_seconds, description, scopes,
                   is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            FROM api_keys
            WHERE key_hash = $1
            LIMIT 1
            "#,
        )
        .bind(key_hash)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".into()))?;

        // --- Cache write (fire-and-forget) ---
        let redis_clone = Arc::clone(&redis);
        let cache_key_clone = cache_key.clone();
        if let Ok(serialized) = serde_json::to_string(&api_key) {
            tokio::spawn(async move {
                if let Ok(mut conn) = redis_clone.get().await {
                    let _: () = conn
                        .set_ex(&cache_key_clone, serialized, API_KEY_CACHE_TTL)
                        .await
                        .unwrap_or_else(|e| {
                            error!(
                                cache_key = %cache_key_clone,
                                error = %e,
                                "Failed to set API key cache key"
                            );
                        });
                }
            });
        }

        Ok(api_key)
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
        let api_key = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT id, user_id, tier, monthly_message_quota,
                   message_retention_seconds, description, scopes,
                   is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            FROM api_keys WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("API key not found".to_string()))?;

        // Cache if Redis available
        if let Some(redis) = redis {
            let cache_key = cache_key_by_id(id);
            if let Ok(serialized) = serde_json::to_string(&api_key) {
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
    pub async fn find_by_owner_paginated<'c, E>(
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
        let keys = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT id, user_id, tier, monthly_message_quota,
                   message_retention_seconds, description, scopes,
                   is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            FROM api_keys 
            WHERE user_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
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
        let keys = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT id, user_id, tier, monthly_message_quota,
                   message_retention_seconds, description, scopes,
                   is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            FROM api_keys 
            ORDER BY created_at DESC 
            LIMIT $1 OFFSET $2
            "#,
        )
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

    /// Updates tier with cache invalidation.
    pub async fn update_tier<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
        new_tier: SubscriptionTier,
    ) -> Result<ApiKey>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let api_key = sqlx::query_as::<_, ApiKey>(
            r#"
            UPDATE api_keys 
            SET 
                tier = $2,
                monthly_message_quota = $3,
                message_retention_seconds = $4,
                rate_limit_per_minute = $5
            WHERE id = $1
            RETURNING id, user_id, tier, monthly_message_quota,
                      message_retention_seconds, description, scopes,
                      is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            "#,
        )
        .bind(id)
        .bind(new_tier)
        .bind(new_tier.default_monthly_quota())
        .bind(new_tier.default_retention_seconds())
        .bind(new_tier.default_rate_limit())
        .fetch_one(exec)
        .await?;

        // Invalidate cache
        Self::invalidate_cache(redis, id).await;

        Ok(api_key)
    }

    /// Deactivate key with cache invalidation.
    pub async fn deactivate<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query("UPDATE api_keys SET is_active = false WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        // Invalidate cache
        Self::invalidate_cache(redis, id).await;

        Ok(())
    }

    /// Reactivate key with cache invalidation.
    pub async fn reactivate<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPoolType>>,
        id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query("UPDATE api_keys SET is_active = true WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        // Invalidate cache
        Self::invalidate_cache(redis, id).await;

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
        E: Executor<'c, Database = Postgres>,
    {
        let api_key = sqlx::query_as::<_, ApiKey>(
            r#"
            UPDATE api_keys 
            SET description = COALESCE($2, description)
            WHERE id = $1
            RETURNING id, user_id, tier, monthly_message_quota,
                      message_retention_seconds, description, scopes,
                      is_active, created_at, expires_at, last_used_at, rate_limit_per_minute
            "#,
        )
        .bind(id)
        .bind(description)
        .fetch_one(exec)
        .await?;

        // Invalidate cache
        Self::invalidate_cache(redis, id).await;

        Ok(api_key)
    }

    /// Hard delete key with cache invalidation.
    pub async fn delete<'c, E>(exec: E, redis: Option<Arc<RedisPoolType>>, id: Uuid) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query("DELETE FROM api_keys WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        // Invalidate cache
        Self::invalidate_cache(redis, id).await;

        Ok(())
    }

    /// Quota check with caching.
    pub async fn check_quota<'c, E>(
        exec: E,
        redis: Arc<RedisPoolType>,
        api_key_id: Uuid,
    ) -> Result<bool>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let api_key = Self::find_by_id(exec.clone(), Some(Arc::clone(&redis)), api_key_id).await?;
        let quota = api_key.monthly_message_quota as i64;
        let month_key = quota_cache_key(api_key_id);

        // Try cache first
        if let Ok(mut conn) = redis.get().await {
            if let Ok(count) = conn.get::<_, i64>(&month_key).await {
                return Ok(count < quota);
            }
        }

        // Cache miss - fetch from DB
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM messages 
            WHERE api_key_id = $1 
              AND created_at >= date_trunc('month', NOW())
            "#,
        )
        .bind(api_key_id)
        .fetch_one(exec)
        .await?;

        // Update cache (fire-and-forget)
        tokio::spawn(async move {
            if let Ok(mut conn) = redis.get().await {
                let _: () = conn
                    .set_ex(&month_key, count, QUOTA_CACHE_TTL)
                    .await
                    .unwrap_or_else(|e| {
                        error!(
                            cache_key = %month_key,
                            error = %e,
                            "Failed to set quota cache key"
                        );
                    });
            }
        });

        Ok(count < quota)
    }

    /// Invalidate all caches for an API key
    async fn invalidate_cache(redis: Option<Arc<RedisPoolType>>, id: Uuid) {
        if let Some(redis) = redis {
            tokio::spawn(async move {
                if let Ok(mut conn) = redis.get().await {
                    let cache_key = cache_key_by_id(id);

                    // 1. Invalidate API key cache
                    let _: () = conn.del(&cache_key).await.unwrap_or_else(|e| {
                        error!(
                            cache_key = %cache_key,
                            error = %e,
                            "Failed to invalidate API key cache"
                        );
                    });

                    // 2. Invalidate quota cache
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
