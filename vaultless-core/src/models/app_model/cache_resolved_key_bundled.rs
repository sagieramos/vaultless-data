use super::dto::*;
use crate::models::{
    ApiKey,
    usage::{MetricGranularity, MetricKey, MetricsConfig, increment_rate_limit_hit_pool},
};
use chrono::Utc;
use std::sync::Arc;

impl CachedResolvedKeyBundle {
    /// Hot-path optimized validation
    pub async fn validate_hot(
        &self,
        redis_pool: Arc<deadpool_redis::Pool>,
    ) -> Result<(), ValidationError> {
        let sk = &self.secret_key_row;
        let app = &self.application;

        // In-memory fast checks
        if !sk.is_active || !app.is_active {
            return Err(ValidationError {
                type_code: ValidationFailureType::Deactivated,
                message: "API key or associated application is deactivated.".into(),
            });
        }

        if let Some(expiry) = sk.expires_at {
            if Utc::now() > expiry {
                return Err(ValidationError {
                    type_code: ValidationFailureType::Expired,
                    message: "API key has expired.".into(),
                });
            }
        }

        // Build Redis keys
        let monthly_key = ApiKey::quota_cache_key(sk.id);
        let now = Utc::now();
        let period_key =
            MetricKey::new(sk.id, now, MetricGranularity::Minute).map_err(|_| ValidationError {
                type_code: ValidationFailureType::Error,
                message: "Failed to create metric key".into(),
            })?;

        // Fetch monthly quota and period metrics
        let mut conn = redis_pool.get().await.map_err(|_| ValidationError {
            type_code: ValidationFailureType::ErrorRedis,
            message: "Failed to get Redis connection".into(),
        })?;

        let results: Vec<Option<i64>> = redis::pipe()
            .atomic()
            .get(&monthly_key)
            .hget(&period_key.as_str(), "messages_sent")
            .hget(&period_key.as_str(), "messages_received")
            .query_async(&mut *conn)
            .await
            .map_err(|_| ValidationError {
                type_code: ValidationFailureType::ErrorRedis,
                message: "Redis query failed".into(),
            })?;

        let monthly_messages = results.get(0).copied().flatten().unwrap_or(0);
        let messages_sent = results.get(1).copied().flatten().unwrap_or(0);
        let messages_received = results.get(2).copied().flatten().unwrap_or(0);
        let total_requests = messages_sent + messages_received;

        // Validate quotas
        if monthly_messages >= sk.monthly_message_quota as i64 {
            return Err(ValidationError {
                type_code: ValidationFailureType::QuotaExhausted,
                message: "API key monthly quota exhausted.".into(),
            });
        }

        if total_requests >= sk.rate_limit_per_minute as i64 {
            let sk_id = sk.id;
            let pool_clone = redis_pool.clone();

            tokio::spawn(async move {
                let _ =
                    increment_rate_limit_hit_pool(&*pool_clone, sk_id, &MetricsConfig::default())
                        .await;
            });
        }

        Ok(())
    }
}
