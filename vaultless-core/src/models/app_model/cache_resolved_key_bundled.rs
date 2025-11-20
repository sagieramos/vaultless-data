use super::dto::*;
use crate::crypto::hash_content;
use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;
use crate::models::usage::{
    MetricGranularity, MetricKey, MetricsConfig, increment_rate_limit_hit_pool,
};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

impl AuthConfig {
    /// Hot-path optimized validation
    pub async fn validate_hot(&self, redis_pool: Arc<RedisPool>) -> Result<()> {
        // 1. In-memory fast checks
        if !self.app_is_active {
            return Err(VaultlessError::Forbidden(
                "Associated application is deactivated.".into(),
            ));
        }
        // 3. QUOTA AND RATE LIMIT CHECKS
        let monthly_key = ApiKey::quota_cache_key(self.sk_id);
        let now = Utc::now();
        let period_key = MetricKey::new(self.sk_id, now, MetricGranularity::Minute)
            .map_err(|e| VaultlessError::Internal(format!("Failed to create metric key: {}", e)))?;

        // Fetch monthly quota and period metrics
        let mut conn = redis_pool.get().await?;

        let results: Vec<Option<i64>> = redis::pipe()
            .atomic()
            .get(&monthly_key)
            .hget(&period_key.as_str(), "messages_sent")
            .hget(&period_key.as_str(), "messages_received")
            .query_async(&mut *conn)
            .await?;

        let monthly_messages = results.get(0).copied().flatten().unwrap_or(0);
        let messages_sent = results.get(1).copied().flatten().unwrap_or(0);
        let messages_received = results.get(2).copied().flatten().unwrap_or(0);
        let total_requests = messages_sent + messages_received;

        // Validate quotas
        let monthly_quota = self.sk_monthly_message_quota.unwrap_or(i32::MAX) as i64;
        if monthly_messages >= monthly_quota {
            return Err(VaultlessError::QuotaExceeded(
                "API key monthly quota exhausted.".into(),
            ));
        }

        // Validate rate limits
        let rate_limit = self.sk_rate_limit_per_minute.unwrap_or(i32::MAX) as i64;
        if total_requests >= rate_limit {
            let sk_id = self.sk_id;
            let pool_clone = redis_pool.clone();

            tokio::spawn(async move {
                let _ =
                    increment_rate_limit_hit_pool(&*pool_clone, sk_id, &MetricsConfig::default())
                        .await;
            });

            return Err(VaultlessError::RateLimitExceeded(
                "API key rate limit exceeded.".into(),
            ));
        }

        Ok(())
    }

    /// Resolve and validate an API key on the hot path
    pub async fn resolve_and_validate<'c, E>(
        exec: E,
        redis_pool: Arc<RedisPool>,
        key_plaintext: &str,
        granularity: &KeyGranularity,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Step 1: Resolve the key based on granularity
        let auth_config = match granularity {
            KeyGranularity::Publishable => {
                super::Application::fetch_auth_config_by_publishable_key(
                    exec.clone(),
                    Some(redis_pool.clone()),
                    key_plaintext,
                )
                .await?
            }
            KeyGranularity::Secret => {
                // For secret keys, we need to hash the key first
                let secret_hash = hash_content(key_plaintext.as_bytes());
                super::Application::fetch_auth_config_by_secret_hash(
                    exec.clone(),
                    Some(redis_pool.clone()),
                    &secret_hash,
                )
                .await?
            }
        };

        // Step 2: Check if the key was found
        let auth_config = auth_config.ok_or_else(|| {
            VaultlessError::NotFound(match granularity {
                KeyGranularity::Publishable => "Publishable key not found.".into(),
                KeyGranularity::Secret => "Secret key not found.".into(),
            })
        })?;

        // Step 3: Run hot validation
        auth_config.validate_hot(redis_pool.clone()).await?;

        // Step 4: Return the validated auth config
        Ok(auth_config)
    }
}
