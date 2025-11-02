use crate::error::{Result, VaultlessError};
use deadpool_redis::{Pool as RedisPool, Runtime as RedisRuntime};
use redis::aio::ConnectionManager;

pub type RedisConn = ConnectionManager;

/// Alias for pooled Redis used by the hot request path.
pub type RedisPoolType = RedisPool;

// =============================================================================
// Redis Connection Management
// =============================================================================

/// Creates a new async Redis connection manager (single connection) used for background tasks.
///
/// # Errors
/// Returns `VaultlessError::Internal` if connection fails.
pub async fn create_redis_conn(redis_url: &str) -> Result<RedisConn> {
    let client =
        redis::Client::open(redis_url).map_err(|e| VaultlessError::Internal(e.to_string()))?;
    ConnectionManager::new(client)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))
}

/// Creates a new deadpool Redis pool for high-throughput request path.
pub fn create_redis_pool(redis_url: &str) -> Result<RedisPoolType> {
    let cfg = deadpool_redis::Config::from_url(redis_url);
    cfg.create_pool(Some(RedisRuntime::Tokio1))
        .map_err(|e| VaultlessError::Internal(e.to_string()))
}
