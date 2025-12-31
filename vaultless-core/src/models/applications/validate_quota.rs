use super::dto::*;
use crate::cache_key;
use crate::crypto::hash_content;
use crate::error::{Result, VaultlessError};
use crate::models::notification::NotificationEventTracker;
use crate::models::usage::application::{
    MetricGranularity, AppMetricKey, record_rate_limit_hit, RecordRateLimitHitInput,
};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

const PUBLISHABLE_KEY_PREFIX: &str = "pk_";
const SECRET_KEY_PREFIX: &str = "sk_";

impl AuthCacheEntry {
    /// Fast-path validation using Redis HASH directly.
    /// Returns Ok(remaining_quota) if validation passes.
    /// Returns Err if quota or rate limit exceeded.
    pub async fn validate_hot(
        &self,
        redis_pool: Arc<RedisPool>,
        sk_id: Uuid,
    ) -> Result<i64> {
        let mut conn = redis_pool.get().await?;

        // Get monthly quota from cache
        let app_id = self.app_id;
        let monthly_quota_key = cache_key!("quota", "app", app_id);

        // Get rate limit key
        let now = Utc::now();
        let period_key = AppMetricKey::new(sk_id, now, MetricGranularity::Minute);

        // Atomic pipeline to fetch both global quota and local rate limit
        let results: Vec<Option<i64>> = redis::pipe()
            .atomic()
            .get(&monthly_quota_key) // App-wide monthly total
            .hget(period_key.as_str(), "messages_sent") // Per-key minute sent
            .hget(period_key.as_str(), "messages_received") // Per-key minute received
            .query_async(&mut *conn)
            .await?;

        let monthly_usage = results.first().copied().flatten().unwrap_or(0);
        let current_min_sent = results.get(1).copied().flatten().unwrap_or(0);
        let current_min_received = results.get(2).copied().flatten().unwrap_or(0);
        let current_min_total = current_min_sent + current_min_received;

        // 1. Validate Monthly Quota (Shared across all keys in the app)
        if monthly_usage >= self.monthly_quota {
            let pool_clone = redis_pool.clone();
            tokio::spawn(async move {
                let _ =
                    NotificationEventTracker::check_and_mark_quota_exceeded(&pool_clone, app_id)
                        .await;
            });

            return Err(VaultlessError::QuotaExceeded(
                "Application monthly message quota exhausted.".into(),
            ));
        }

        // 2. Validate Rate Limit (Specific to this API Key)
        if current_min_total >= self.rate_limit_per_minute as i64 {
            let sk_id_for_metric = sk_id;
            let pool_clone = redis_pool.clone();

            tokio::spawn(async move {
                let _ =
                    record_rate_limit_hit(
                        &pool_clone,
                        RecordRateLimitHitInput::new(uuid::Uuid::new_v4(), app_id),
                        None,
                    )
                    .await;

                let _ =
                    NotificationEventTracker::increment_rate_limit_hits(&pool_clone, sk_id_for_metric).await;
            });

            return Err(VaultlessError::RateLimitExceeded(
                "API key rate limit exceeded.".into(),
            ));
        }

        // Return remaining quota
        Ok(self.monthly_quota - monthly_usage)
    }
}

impl ApplicationKeyView {
    /// Fast-path validation using Redis cache only (no DB fetch)
    /// Returns Ok(()) if cached entry exists and passes validation
    /// Returns Err if validation fails or cache miss (caller should fetch from DB)
    async fn try_validate_from_cache(
        redis_pool: Arc<RedisPool>,
        api_key: &str,
    ) -> Result<()> {
        let cache_key = if api_key.starts_with(PUBLISHABLE_KEY_PREFIX) {
            publishable_key_resolution_cache_key(api_key)
        } else if api_key.starts_with(SECRET_KEY_PREFIX) {
            secret_key_resolution_cache_key(&hash_content(api_key.as_bytes()))
        } else {
            return Err(VaultlessError::Unauthorized(
                "API key must start with 'pk_' or 'sk_' prefix.".into(),
            ));
        };

        let mut conn = redis_pool.get().await?;

        // HGETALL for O(1) access without JSON parsing
        let vals: HashMap<String, String> = conn.hgetall(&cache_key).await?;

        if vals.is_empty() {
            return Err(VaultlessError::NotFound("Cache miss".into())); // Signal caller to fetch from DB
        }

        let auth_entry = AuthCacheEntry::from_redis(vals)
            .ok_or_else(|| VaultlessError::Internal("Invalid cache format".into()))?;

        // Check if app is active
        if !auth_entry.is_active {
            return Err(VaultlessError::Forbidden("Application is deactivated.".into()));
        }

        // Validate quotas and rate limits
        auth_entry.validate_hot(redis_pool, auth_entry.sk_id).await?;

        Ok(())
    }

    /// Validate quotas and rate limits for this application key
    pub async fn validate_hot(&self, redis_pool: Arc<RedisPool>) -> Result<()> {
        if !self.app_is_active {
            return Err(VaultlessError::Forbidden(
                "Associated application is deactivated.".into(),
            ));
        }

        // --- QUOTA LOGIC (Application Level) ---
        let monthly_quota_key = cache_key!("quota", "app", self.app_id);

        // --- RATE LIMIT LOGIC (Key Level) ---
        let now = Utc::now();
        let period_key = AppMetricKey::new(self.sk_id, now, MetricGranularity::Minute);

        let mut conn = redis_pool.get().await?;

        // Atomic pipeline to fetch both global quota and local rate limit
        let results: Vec<Option<i64>> = redis::pipe()
            .atomic()
            .get(&monthly_quota_key) // App-wide monthly total
            .hget(period_key.as_str(), "messages_sent") // Per-key minute sent
            .hget(period_key.as_str(), "messages_received") // Per-key minute received
            .query_async(&mut *conn)
            .await?;

        let monthly_usage = results.first().copied().flatten().unwrap_or(0);
        let current_min_sent = results.get(1).copied().flatten().unwrap_or(0);
        let current_min_received = results.get(2).copied().flatten().unwrap_or(0);
        let current_min_total = current_min_sent + current_min_received;

        // 1. Validate Monthly Quota (Shared across all keys in the app)
        if monthly_usage >= self.sub_monthly_message_quota {
            let app_id = self.app_id;
            let pool_clone = redis_pool.clone();
            tokio::spawn(async move {
                let _ =
                    NotificationEventTracker::check_and_mark_quota_exceeded(&pool_clone, app_id)
                        .await;
            });

            return Err(VaultlessError::QuotaExceeded(
                "Application monthly message quota exhausted.".into(),
            ));
        }

        // 2. Validate Rate Limit (Specific to this API Key)
        if current_min_total >= self.sub_rate_limit_per_minute as i64 {
            let _ = record_rate_limit_hit(
                &redis_pool,
                RecordRateLimitHitInput::new(Uuid::new_v4(), self.app_id),
                None,
            )
            .await;

            return Err(VaultlessError::RateLimitExceeded(
                "API key rate limit exceeded.".into(),
            ));
        }

        Ok(())
    }

    /// Resolve and validate an API key (hot path optimized)
    /// First tries cache-only validation, falls back to DB fetch if cache miss
    pub async fn resolve_and_validate<'c, E>(
        exec: E,
        redis_pool: Arc<RedisPool>,
        api_key: &str,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Try fast path: validate from cache only (no DB fetch)
        if Self::try_validate_from_cache(redis_pool.clone(), api_key).await.is_ok() {
            // Cache hit and validation passed - now fetch full data from DB
            // We need full data including app_meta for potential integrity checks
            let key_type = if api_key.starts_with(PUBLISHABLE_KEY_PREFIX) {
                KeyGranularity::Publishable
            } else {
                KeyGranularity::Secret
            };

            let auth_config = match key_type {
                KeyGranularity::Publishable => {
                    super::Application::fetch_full_auth_by_publishable_key(exec, api_key).await?
                }
                KeyGranularity::Secret => {
                    let secret_hash = hash_content(api_key.as_bytes());
                    super::Application::fetch_full_auth_by_secret_hash(exec, &secret_hash).await?
                }
            };

            return auth_config.ok_or_else(|| {
                VaultlessError::NotFound(match key_type {
                    KeyGranularity::Publishable => "Publishable key not found.".into(),
                    KeyGranularity::Secret => "Secret key not found.".into(),
                })
            });
        }

        // Cache miss or validation failed - fetch from DB and validate
        let key_type = if api_key.starts_with(PUBLISHABLE_KEY_PREFIX) {
            KeyGranularity::Publishable
        } else if api_key.starts_with(SECRET_KEY_PREFIX) {
            KeyGranularity::Secret
        } else {
            return Err(VaultlessError::Unauthorized(
                "API key must start with 'pk_' or 'sk_' prefix.".into(),
            ));
        };

        let auth_config = match key_type {
            KeyGranularity::Publishable => {
                super::Application::fetch_auth_config_by_publishable_key(
                    exec,
                    Some(redis_pool.clone()),
                    api_key,
                )
                .await?
            }
            KeyGranularity::Secret => {
                let secret_hash = hash_content(api_key.as_bytes());
                super::Application::fetch_auth_config_by_secret_hash(
                    exec,
                    Some(redis_pool.clone()),
                    &secret_hash,
                )
                .await?
            }
        };

        let auth_config = auth_config.ok_or_else(|| {
            VaultlessError::NotFound(match key_type {
                KeyGranularity::Publishable => "Publishable key not found.".into(),
                KeyGranularity::Secret => "Secret key not found.".into(),
            })
        })?;

        // Validate quota and rate limits
        auth_config.validate_hot(redis_pool).await?;

        Ok(auth_config)
    }
}
