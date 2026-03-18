use super::dto::*;
use super::resolve::AuthLookup;
use crate::cache_key;
use crate::crypto::hash_content;
use crate::error::{Result, VaultlessError};
use crate::models::notification::NotificationEventTracker;
use crate::models::usage::application::{
    MetricGranularity, AppMetricKey, record_rate_limit_hit, RecordRateLimitHitInput,
};
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;
use once_cell::sync::Lazy;

const PUBLISHABLE_KEY_PREFIX: &str = "pk_";
const SECRET_KEY_PREFIX: &str = "sk_";

// Pre-load scripts for performance (avoid sending scripts to Redis every request)
static QUOTA_RATE_LIMIT_SCRIPT: Lazy<redis::Script> = Lazy::new(|| {
    redis::Script::new(include_str!("../../scripts/quota_rate_limit_check.lua"))
});

static LOCK_RELEASE_SCRIPT: Lazy<redis::Script> = Lazy::new(|| {
    redis::Script::new(r#"
        if redis.call("GET", KEYS[1]) == ARGV[1] then
            return redis.call("DEL", KEYS[1])
        else
            return 0
        end
    "#)
});

/// Parse and validate API key prefix, returning the key type
fn parse_key_type(api_key: &str) -> Result<KeyGranularity> {
    if api_key.starts_with(PUBLISHABLE_KEY_PREFIX) {
        Ok(KeyGranularity::Publishable)
    } else if api_key.starts_with(SECRET_KEY_PREFIX) {
        Ok(KeyGranularity::Secret)
    } else {
        Err(VaultlessError::Unauthorized(
            "API key must start with 'pk_' or 'sk_' prefix.".into(),
        ))
    }
}

/// Get cache key for this API key
fn get_cache_key(api_key: &str) -> Result<String> {
    let key_type = parse_key_type(api_key)?;
    match key_type {
        KeyGranularity::Publishable => Ok(publishable_key_resolution_cache_key(api_key)),
        KeyGranularity::Secret => Ok(secret_key_resolution_cache_key(&hash_content(api_key.as_bytes()))),
    }
}

/// Validate quotas and rate limits atomically in a single Redis operation
/// Returns Ok(period_usage) if validation passes
async fn validate_quotas_and_limits(
    redis_pool: &Arc<RedisPool>,
    app_id: Uuid,
    sk_id: Uuid,
    period_end: Option<i64>,
    period_quota: i64,
    rate_limit_per_minute: i64,
) -> Result<i64> {
    // Validate subscription expiry (no Redis operation needed)
    if let Some(period_end) = period_end {
        let period_end_utc = DateTime::from_timestamp(period_end, 0)
            .ok_or_else(|| VaultlessError::Internal("Invalid period_end timestamp".into()))?;
        if Utc::now() > period_end_utc {
            return Err(VaultlessError::Forbidden(
                "Subscription has expired.".into(),
            ));
        }
    }

    // Prepare keys for atomic quota + rate limit check
    let now = Utc::now();
    let period_key = AppMetricKey::new(sk_id, now, MetricGranularity::Minute);
    let rate_limit_key = period_key.as_str();
    let period_quota_key = cache_key!("quota", "app", app_id);

    // Execute combined quota and rate limit check atomically in Lua
    // Rate limit check first (cheap + fast fail protects system early), then quota check
    // Returns {status_code, period_usage} with zero string parsing overhead
    let mut conn = redis_pool.get().await?;
    let result: Vec<i64> = QUOTA_RATE_LIMIT_SCRIPT
        .key(rate_limit_key)
        .key(&period_quota_key)
        .arg(rate_limit_per_minute)
        .arg(period_quota)
        .invoke_async(&mut *conn)
        .await?;

    // Parse structured response: [status_code, period_usage]
    // No string parsing overhead - integers from Lua directly
    if result.len() != 2 {
        return Err(VaultlessError::Internal("Invalid script response format".into()));
    }

    let status = result[0];
    let period_usage = result[1];

    match status {
        1 => {
            // Validation passed, return current usage
            Ok(period_usage)
        }
        2 => {
            // Period quota exceeded - spawn notification task
            let pool_clone = redis_pool.clone();
            tokio::spawn(async move {
                if let Err(e) = NotificationEventTracker::check_and_mark_quota_exceeded(&pool_clone, app_id).await {
                    tracing::error!("Failed to track quota exceeded notification: {:?}", e);
                }
            });

            Err(VaultlessError::QuotaExceeded(
                "Application message quota exhausted.".into(),
            ))
        }
        3 => {
            // Rate limit exceeded (checked first for fast fail and better UX)
            let pool_clone = redis_pool.clone();
            tokio::spawn(async move {
                if let Err(e) = record_rate_limit_hit(
                    &pool_clone,
                    RecordRateLimitHitInput::new(Uuid::new_v4(), app_id),
                    None,
                ).await {
                    tracing::error!("Failed to record rate limit hit: {:?}", e);
                }

                if let Err(e) = NotificationEventTracker::increment_rate_limit_hits(&pool_clone, sk_id).await {
                    tracing::error!("Failed to increment rate limit hits notification: {:?}", e);
                }
            });

            Err(VaultlessError::RateLimitExceeded(
                "API key rate limit exceeded.".into(),
            ))
        }
        _ => Err(VaultlessError::Internal(format!("Unknown script status code: {}", status))),
    }
}

impl AuthCacheEntry {
    /// Validate quotas and rate limits for this cached entry
    pub async fn validate_hot(
        &self,
        redis_pool: Arc<RedisPool>,
        sk_id: Uuid,
    ) -> Result<i64> {
        // Check if app is active
        if !self.is_active {
            return Err(VaultlessError::Forbidden("Application is deactivated.".into()));
        }

        let period_usage = validate_quotas_and_limits(
            &redis_pool,
            self.app_id,
            sk_id,
            self.period_end,
            self.monthly_quota,
            self.rate_limit_per_minute as i64,
        )
        .await?;

        // Return remaining quota
        Ok(self.monthly_quota - period_usage)
    }
}

impl ApplicationKeyView {
    /// Validate quotas and rate limits for this application key
    pub async fn validate_hot(&self, redis_pool: Arc<RedisPool>) -> Result<()> {
        if !self.app_is_active {
            return Err(VaultlessError::Forbidden(
                "Associated application is deactivated.".into(),
            ));
        }

        let period_end = self.sub_period_end.map(|dt| dt.timestamp());

        validate_quotas_and_limits(
            &redis_pool,
            self.app_id,
            self.sk_id,
            period_end,
            self.sub_message_quota,
            self.sub_rate_limit_per_minute as i64,
        )
        .await?;

        Ok(())
    }

    /// Validate an API key from cache
    /// Returns Ok(true) if valid, Err if invalid or not found
    pub async fn validate(
        redis_pool: Arc<RedisPool>,
        api_key: &str,
    ) -> Result<bool> {
        // Get cache key
        let cache_key = get_cache_key(api_key)?;
        let mut conn = redis_pool.get().await?;

        // Fetch from cache
        let vals: HashMap<String, String> = conn.hgetall(&cache_key).await?;
        if vals.is_empty() {
            return Err(VaultlessError::CacheMiss);
        }

        // Parse cache entry
        let auth_entry = AuthCacheEntry::from_redis(vals)
            .map_err(|e| VaultlessError::Internal(format!("Invalid cache format: {}", e)))?;

        // Validate
        auth_entry.validate_hot(redis_pool, auth_entry.sk_id).await.map(|_| true)
    }

/// Validate an API key with DB fallback on cache miss
/// Uses distributed locking to prevent thundering herd on cache misses
/// Returns Ok(true) if valid, Err if invalid or not found
pub async fn validate_with_fallback<'c, E>(
    exec: E,
    redis_pool: Arc<RedisPool>,
    api_key: &str,
) -> Result<bool>
where
    E: Executor<'c, Database = Postgres> + Clone,
{
    // Try cache first
    match Self::validate(redis_pool.clone(), api_key).await {
        Ok(_) => return Ok(true), // Cache hit and valid
        Err(VaultlessError::CacheMiss) => {
            tracing::info!("Cache miss for API key validation, attempting DB fallback with lock");
        }
        Err(e) => return Err(e), // Other errors (invalid key, etc.)
    }

    // Acquire distributed lock to prevent thundering herd
    // Use hashed API key for security (never store raw secrets as keys)
    let lock_key = format!("lock:api_key:{}", hash_content(api_key.as_bytes()));
    let lock_value = Uuid::new_v4().to_string(); // Unique lock token
    let lock_ttl_seconds = 5; // 5 second lock

    let mut conn = redis_pool.get().await?;
    let lock_acquired: bool = redis::cmd("SET")
        .arg(&lock_key)
        .arg(&lock_value)
        .arg("NX")
        .arg("EX")
        .arg(lock_ttl_seconds)
        .query_async(&mut *conn)
        .await
        .unwrap_or(false);

    if !lock_acquired {
        // Another request is already fetching from DB, retry cache with backoff
        tracing::debug!("DB fallback lock held by another request, waiting and retrying cache");

        // Retry loop to reduce cache stampede under heavy load
        for attempt in 0..3 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50 * (attempt + 1) as u64)).await;

            match Self::validate(redis_pool.clone(), api_key).await {
                Ok(_) => return Ok(true), // Cache populated by other request
                Err(VaultlessError::CacheMiss) => {
                    // Still missing, continue retry loop
                    tracing::debug!("Cache still empty after retry {}, continuing", attempt + 1);
                }
                Err(e) => return Err(e),
            }
        }

        // All retries failed, proceed with our own DB lookup (lock may have expired)
        tracing::debug!("Cache still empty after all retries, proceeding with DB lookup");
    }

    // We have the lock (or retries exhausted), do DB lookup with timeout
    let result = tokio::time::timeout(
        tokio::time::Duration::from_secs(2),
        Self::validate_from_db(exec, redis_pool.clone(), api_key)
    ).await
    .map_err(|_| VaultlessError::Internal("DB fallback timeout".into()))?;

    // Safe lock release using Lua script (prevents deleting other request's lock)
    if lock_acquired {
        let _: i32 = LOCK_RELEASE_SCRIPT
            .key(&lock_key)
            .arg(&lock_value)
            .invoke_async(&mut conn)
            .await
            .unwrap_or(0);
    }

    result
}

/// Validate API key directly from database (internal helper)
async fn validate_from_db<'c, E>(
    exec: E,
    redis_pool: Arc<RedisPool>,
    api_key: &str,
) -> Result<bool>
where
    E: Executor<'c, Database = Postgres> + Clone,
{
    let key_type = parse_key_type(api_key)?;

    // Create secret_hash if needed (must live longer than the match)
    let secret_hash = if matches!(key_type, KeyGranularity::Secret) {
        Some(hash_content(api_key.as_bytes()))
    } else {
        None
    };

    let auth_config = match key_type {
        KeyGranularity::Publishable => {
            super::Application::fetch_auth_internal(
                exec,
                Some(redis_pool.clone()),
                AuthLookup::Publishable(api_key),
                true,
            )
            .await?
            .ok_or_else(|| {
                VaultlessError::Unauthorized("API key not found.".into())
            })?
        }
        KeyGranularity::Secret => {
            super::Application::fetch_auth_internal(
                exec,
                Some(redis_pool.clone()),
                AuthLookup::SecretHash(secret_hash.as_ref().unwrap()),
                true,
            )
            .await?
            .ok_or_else(|| {
                VaultlessError::Unauthorized("API key not found.".into())
            })?
        }
    };

    // Validate quota and rate limits
    if !auth_config.app_is_active {
        return Err(VaultlessError::Forbidden(
            "Associated application is deactivated.".into(),
        ));
    }

    let period_end = auth_config.sub_period_end.map(|dt| dt.timestamp());

    validate_quotas_and_limits(
        &redis_pool,
        auth_config.app_id,
        auth_config.sk_id,
        period_end,
        auth_config.sub_message_quota,
        auth_config.sub_rate_limit_per_minute as i64,
    )
    .await?;

    Ok(true)
}
}
