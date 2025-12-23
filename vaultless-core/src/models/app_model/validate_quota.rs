use super::dto::*;
use crate::cache_key;
use crate::crypto::hash_content;
use crate::error::{Result, VaultlessError};
use crate::models::notification::NotificationEventTracker;
use crate::models::usage::{
    MetricGranularity, MetricKey, record_rate_limit_hit, RecordRateLimitHitInput,
};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

const PUBLISHABLE_KEY_PREFIX: &str = "pk_";
const SECRET_KEY_PREFIX: &str = "sk_";

impl ApplicationKeyView {
    /// Validates quotas and rate limits in the fast path (Redis-only)
    pub async fn validate_hot(&self, redis_pool: Arc<RedisPool>) -> Result<()> {
        if !self.app_is_active {
            return Err(VaultlessError::Forbidden(
                "Associated application is deactivated.".into(),
            ));
        }

        // --- QUOTA LOGIC (Application Level) ---
        // Quotas are now shared across the application via app_id
        let monthly_quota_key = cache_key!("quota", "app", self.app_id);

        // --- RATE LIMIT LOGIC (Key Level) ---
        // Rate limits remain specific to the individual API Key
        let now = Utc::now();
        let period_key = MetricKey::new(self.sk_id, now, MetricGranularity::Minute)
            .map_err(|e| VaultlessError::Internal(format!("Failed to create metric key: {}", e)))?;

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
                // Notifications are now tracked at the App/Subscription level
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
            let sk_id = self.sk_id;
            let app_id = self.app_id;
            let pool_clone = redis_pool.clone();

            tokio::spawn(async move {
                // Increment rate limit hit metrics for this specific key
                let _ =
                    record_rate_limit_hit(
                        &pool_clone,
                        RecordRateLimitHitInput::new(uuid::Uuid::new_v4(), app_id),
                        None,
                    )
                    .await;

                let _ =
                    NotificationEventTracker::increment_rate_limit_hits(&pool_clone, sk_id).await;
            });

            return Err(VaultlessError::RateLimitExceeded(
                "API key rate limit exceeded.".into(),
            ));
        }

        Ok(())
    }

    pub async fn resolve_and_validate<'c, E>(
        exec: E,
        redis_pool: Arc<RedisPool>,
        api_key: &str,
    ) -> Result<Self>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
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
                    exec.clone(),
                    Some(redis_pool.clone()),
                    api_key,
                )
                .await?
            }
            KeyGranularity::Secret => {
                let secret_hash = hash_content(api_key.as_bytes());
                super::Application::fetch_auth_config_by_secret_hash(
                    exec.clone(),
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

        // Run the hot-path validation (Quota & Rate Limit)
        auth_config.validate_hot(redis_pool.clone()).await?;

        Ok(auth_config)
    }
}
