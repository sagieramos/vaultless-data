use crate::error::{Result, VaultlessError};
use crate::models::ApiKey;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct QuotaStatus {
    pub limit: i64,
    pub used: i64,
    pub remaining: i64,
    pub percentage_used: f64,
    pub is_exceeded: bool,
}

pub async fn get_live_usage(
    redis_pool: Arc<super::usage::RedisPoolType>,
    api_key_id: Uuid,
    quota_limit: i64,
) -> Result<QuotaStatus> {
    let quota_key = ApiKey::quota_cache_key(api_key_id);

    // 1. ACQUIRE CONNECTION from the pool
    // This is now the responsibility of the utility function.
    let mut conn = redis_pool.get().await.map_err(|e| {
        VaultlessError::Internal(format!("Failed to acquire Redis connection: {}", e))
    })?;

    // 2. Execute command using the acquired connection reference
    let monthly_count: Option<i64> = redis::cmd("GET")
        .arg(&quota_key)
        .query_async(&mut conn)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    // When 'conn' goes out of scope here, it is automatically returned to the pool.

    let used = monthly_count.unwrap_or(0);
    let remaining = quota_limit.saturating_sub(used);
    let percentage_used = if quota_limit > 0 {
        (used as f64 / quota_limit as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    Ok(QuotaStatus {
        limit: quota_limit,
        used,
        remaining,
        percentage_used,
        is_exceeded: used >= quota_limit,
    })
}
