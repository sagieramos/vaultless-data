use deadpool_redis::{Pool as RedisPool, redis::AsyncCommands};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

use crate::middleware::error::ApiError;

/// Cache service for Dragonfly/Redis operations
pub struct CacheService {
    pool: RedisPool,
    default_ttl: Duration,
}

impl CacheService {
    pub fn new(pool: RedisPool, default_ttl_secs: u64) -> Self {
        Self {
            pool,
            default_ttl: Duration::from_secs(default_ttl_secs),
        }
    }

    /// Get value from cache
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let data: Option<String> = conn.get(key).await.map_err(|e| {
            tracing::error!("Cache get error for key '{}': {}", key, e);
            ApiError::internal_server_error("Cache read error")
        })?;

        match data {
            Some(json_str) => {
                let value = serde_json::from_str(&json_str).map_err(|e| {
                    tracing::error!("Cache deserialization error: {}", e);
                    ApiError::internal_server_error("Cache data corrupted")
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Set value in cache with default TTL
    pub async fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<(), ApiError> {
        self.set_with_ttl(key, value, self.default_ttl).await
    }

    /// Set value in cache with custom TTL
    pub async fn set_with_ttl<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl: Duration,
    ) -> Result<(), ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let json_str = serde_json::to_string(value).map_err(|e| {
            tracing::error!("Cache serialization error: {}", e);
            ApiError::internal_server_error("Cache serialization failed")
        })?;

        conn.set_ex::<_, _, ()>(key, json_str, ttl.as_secs())
            .await
            .map_err(|e| {
                tracing::error!("Cache set error for key '{}': {}", key, e);
                ApiError::internal_server_error("Cache write error")
            })?;

        Ok(())
    }

    /// Delete key from cache
    pub async fn delete(&self, key: &str) -> Result<(), ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let deleted: usize = conn.del(key).await.map_err(|e| {
            tracing::error!("Cache delete error for key '{}': {}", key, e);
            ApiError::internal_server_error("Cache delete error")
        })?;

        tracing::info!("Deleted {} key(s) for '{}'", deleted, key);
        Ok(())
    }

    /// Check if key exists
    pub async fn exists(&self, key: &str) -> Result<bool, ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let exists: bool = conn.exists(key).await.map_err(|e| {
            tracing::error!("Cache exists check error for key '{}': {}", key, e);
            ApiError::internal_server_error("Cache check error")
        })?;

        Ok(exists)
    }

    /// Increment counter (for rate limiting)
    pub async fn incr(&self, key: &str) -> Result<i64, ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let count: i64 = conn.incr(key, 1).await.map_err(|e| {
            tracing::error!("Cache incr error for key '{}': {}", key, e);
            ApiError::internal_server_error("Cache increment error")
        })?;

        Ok(count)
    }

    /// Set expiration on existing key
    pub async fn expire(&self, key: &str, ttl: Duration) -> Result<(), ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let expired: bool = conn.expire(key, ttl.as_secs() as i64).await.map_err(|e| {
            tracing::error!("Cache expire error for key '{}': {}", key, e);
            ApiError::internal_server_error("Cache expire error")
        })?;

        if !expired {
            tracing::warn!("TTL was not applied for key '{}'", key);
        }

        Ok(())
    }

    /// Get multiple keys at once
    pub async fn mget<T: DeserializeOwned>(
        &self,
        keys: &[String],
    ) -> Result<Vec<Option<T>>, ApiError> {
        if keys.is_empty() {
            return Ok(vec![]);
        }

        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let data: Vec<Option<String>> = conn.get(keys).await.map_err(|e| {
            tracing::error!("Cache mget error: {}", e);
            ApiError::internal_server_error("Cache read error")
        })?;

        let mut results = Vec::with_capacity(data.len());
        for item in data {
            match item {
                Some(json_str) => {
                    let value = serde_json::from_str(&json_str).map_err(|e| {
                        tracing::error!("Cache deserialization error: {}", e);
                        ApiError::internal_server_error("Cache data corrupted")
                    })?;
                    results.push(Some(value));
                }
                None => results.push(None),
            }
        }

        Ok(results)
    }
}

/// Generate cache key for API key
pub fn api_key_cache_key(key_hash: &str) -> String {
    format!("api_key:{}", key_hash)
}

/// Generate cache key for message list
pub fn message_list_cache_key(recipient_id: &str) -> String {
    format!("messages:{}:list", recipient_id)
}

/// Generate cache key for rate limit
pub fn rate_limit_cache_key(api_key_id: &str, window: &str) -> String {
    format!("rate_limit:{}:{}", api_key_id, window)
}
