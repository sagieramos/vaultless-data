use deadpool_redis::{Pool as RedisPool, redis::AsyncCommands};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::middleware::error::ApiError;

/// Rate limiter using sliding window algorithm
pub struct RateLimiter {
    pool: RedisPool,
}

/// Rate limit result
#[derive(Debug, Clone)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub limit: i64,
    pub remaining: i64,
    pub reset_at: u64,
    pub retry_after: Option<u64>,
}

impl RateLimiter {
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    /// Check if request is allowed using sliding window
    pub async fn check_rate_limit(
        &self,
        key: &str,
        limit: i64,
        window_secs: u64,
    ) -> Result<RateLimitResult, ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Rate limiter connection error: {}", e);
            ApiError::internal_server_error("Rate limiter unavailable")
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let window_start = now.saturating_sub(window_secs);
        let cache_key = format!("ratelimit:{}", key);

        // Improved Lua script with proper sliding window
        let lua_script = r#"
            local key = KEYS[1]
            local now = tonumber(ARGV[1])
            local window_start = tonumber(ARGV[2])
            local limit = tonumber(ARGV[3])
            local window_secs = tonumber(ARGV[4])
            
            -- Remove ALL entries outside the current window
            redis.call('ZREMRANGEBYSCORE', key, '-inf', window_start)
            
            -- Count current requests in the window
            local current_count = redis.call('ZCARD', key)
            
            -- Check if under limit
            if current_count < limit then
                -- Add current request with unique identifier (timestamp + random)
                local unique_id = now .. ':' .. math.random(1000000)
                redis.call('ZADD', key, now, unique_id)
                
                -- Set expiration to window duration + buffer
                redis.call('EXPIRE', key, window_secs + 60)
                
                -- Return: allowed=1, remaining count
                return {1, limit - current_count - 1, now + window_secs}
            else
                -- Get the oldest request in window to calculate reset time
                local oldest = redis.call('ZRANGE', key, 0, 0, 'WITHSCORES')
                local reset_at = now + window_secs
                
                if #oldest > 0 then
                    reset_at = tonumber(oldest[2]) + window_secs
                end
                
                -- Return: allowed=0, remaining=0, reset_at
                return {0, 0, reset_at}
            end
        "#;

        let result: Vec<i64> = redis::Script::new(lua_script)
            .key(&cache_key)
            .arg(now)
            .arg(window_start)
            .arg(limit)
            .arg(window_secs)
            .invoke_async(&mut *conn)
            .await
            .map_err(|e| {
                tracing::error!("Rate limit check error: {}", e);
                ApiError::internal_server_error("Rate limit check failed")
            })?;

        let allowed = result[0] == 1;
        let remaining = result[1];
        let reset_at = result[2] as u64;

        let retry_after = if !allowed {
            Some(reset_at.saturating_sub(now))
        } else {
            None
        };

        tracing::debug!(
            key = %key,
            allowed = allowed,
            remaining = remaining,
            limit = limit,
            "Rate limit check"
        );

        Ok(RateLimitResult {
            allowed,
            limit,
            remaining,
            reset_at,
            retry_after,
        })
    }

    /// Check rate limit for API key
    pub async fn check_api_key_limit(
        &self,
        api_key_id: Uuid,
        requests_per_minute: i32,
    ) -> Result<RateLimitResult, ApiError> {
        let key = format!("apikey:{}", api_key_id);
        self.check_rate_limit(&key, requests_per_minute as i64, 60)
            .await
    }

    /// Check rate limit by IP address
    pub async fn check_ip_limit(&self, ip: &str, limit: i64) -> Result<RateLimitResult, ApiError> {
        let key = format!("ip:{}", ip);
        self.check_rate_limit(&key, limit, 60).await
    }

    /// Check rate limit for specific endpoint
    pub async fn check_endpoint_limit(
        &self,
        api_key_id: Uuid,
        endpoint: &str,
        limit: i64,
        window_secs: u64,
    ) -> Result<RateLimitResult, ApiError> {
        let key = format!("endpoint:{}:{}", api_key_id, endpoint);
        self.check_rate_limit(&key, limit, window_secs).await
    }

    /// Record rate limit violation
    pub async fn record_violation(&self, api_key_id: Uuid) -> Result<(), ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Rate limiter connection error: {}", e);
            ApiError::internal_server_error("Rate limiter unavailable")
        })?;

        let key = format!("violations:{}", api_key_id);
        let _: () = conn.incr(&key, 1).await.map_err(|e| {
            tracing::error!("Failed to record violation: {}", e);
            ApiError::internal_server_error("Failed to record violation")
        })?;

        // Expire after 24 hours
        let _: () = conn.expire(&key, 86400).await.map_err(|e| {
            tracing::error!("Failed to set expiration: {}", e);
            ApiError::internal_server_error("Failed to set expiration")
        })?;

        Ok(())
    }

    /// Get violation count for API key
    pub async fn get_violation_count(&self, api_key_id: Uuid) -> Result<i64, ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Rate limiter connection error: {}", e);
            ApiError::internal_server_error("Rate limiter unavailable")
        })?;

        let key = format!("violations:{}", api_key_id);
        let count: i64 = conn.get(&key).await.unwrap_or(0);

        Ok(count)
    }

    /// Reset rate limit for API key (admin function)
    pub async fn reset_limit(&self, api_key_id: Uuid) -> Result<(), ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Rate limiter connection error: {}", e);
            ApiError::internal_server_error("Rate limiter unavailable")
        })?;

        let key = format!("ratelimit:apikey:{}", api_key_id);
        let _: () = conn.del(&key).await.map_err(|e| {
            tracing::error!("Failed to reset limit: {}", e);
            ApiError::internal_server_error("Failed to reset limit")
        })?;

        Ok(())
    }

    /// Get current usage for API key
    pub async fn get_current_usage(&self, api_key_id: Uuid) -> Result<CurrentUsage, ApiError> {
        let mut conn = self.pool.get().await.map_err(|e| {
            tracing::error!("Rate limiter connection error: {}", e);
            ApiError::internal_server_error("Rate limiter unavailable")
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let window_start = now - 60; // Last minute
        let key = format!("ratelimit:apikey:{}", api_key_id);

        // Remove old entries
        let _: () = conn
            .zrembyscore(&key, "-inf", window_start)
            .await
            .unwrap_or(());

        // Count current requests
        let count: i64 = conn.zcard(&key).await.unwrap_or(0);

        Ok(CurrentUsage {
            requests_in_window: count,
            window_start,
            window_end: now,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CurrentUsage {
    pub requests_in_window: i64,
    pub window_start: u64,
    pub window_end: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_result() {
        let result = RateLimitResult {
            allowed: true,
            limit: 100,
            remaining: 99,
            reset_at: 1697385600,
            retry_after: None,
        };

        assert!(result.allowed);
        assert_eq!(result.remaining, 99);
        assert!(result.retry_after.is_none());
    }

    #[test]
    fn test_rate_limit_exceeded() {
        let result = RateLimitResult {
            allowed: false,
            limit: 100,
            remaining: 0,
            reset_at: 1697385600,
            retry_after: Some(30),
        };

        assert!(!result.allowed);
        assert_eq!(result.remaining, 0);
        assert_eq!(result.retry_after, Some(30));
    }
}
