//! Configuration and constants for the usage metrics system.

use crate::cache_key;
use deadpool_redis::Pool as RedisPool;
use once_cell::sync::Lazy;

// =============================================================================
// Type Aliases
// =============================================================================

/// Alias for pooled Redis used by all operations
pub type RedisPoolType = RedisPool;

// =============================================================================
// Constants
// =============================================================================

/// Default maximum number of keys to flush in a single transaction
pub const DEFAULT_MAX_BATCH_SIZE: usize = 1000;

/// Default Redis key TTL for metric hashes (2 hours)
pub const DEFAULT_METRIC_TTL_SECS: u64 = 7200;

/// Default flush interval in seconds
pub const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 300; // 5 minutes

/// Redis operation timeout in seconds
pub const REDIS_OPERATION_TIMEOUT_SECS: u64 = 30;

/// Set name for tracking active metric keys (application-level)
pub static ACTIVE_KEYS_SET: Lazy<String> = Lazy::new(|| cache_key!("metric", "app", "active_keys"));

/// Set name for tracking active client metric keys
pub static ACTIVE_CLIENT_KEYS_SET: Lazy<String> = Lazy::new(|| cache_key!("metric", "client", "active_keys"));

/// Field name to mark a key as being processed
pub const PROCESSING_FLAG: &str = "_processing";

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for the metrics collection and flushing system
#[derive(Clone, Debug)]
pub struct MetricsConfig {
    /// Maximum number of keys to flush in a single transaction
    pub max_batch_size: usize,
    /// TTL for Redis metric hashes in seconds
    pub metric_ttl_secs: u64,
    /// Interval between flush cycles in seconds
    pub flush_interval_secs: u64,
    /// Timeout for individual Redis operations in seconds
    pub redis_operation_timeout_secs: u64,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            metric_ttl_secs: DEFAULT_METRIC_TTL_SECS,
            flush_interval_secs: DEFAULT_FLUSH_INTERVAL_SECS,
            redis_operation_timeout_secs: REDIS_OPERATION_TIMEOUT_SECS,
        }
    }
}

// =============================================================================
// Engine Configurations
// =============================================================================

/// Configuration for application-level usage engine
#[derive(Debug, Clone)]
pub struct UsageEngineConfig {
    /// TTL for counted keys (prevents replay, e.g., 1 hour)
    pub counted_ttl_secs: i64,
    /// TTL for monthly quota keys (35 days)
    pub monthly_ttl_secs: i64,
    /// TTL for hourly metric keys (2 hours)
    pub hourly_ttl_secs: i64,
    /// Redis operation timeout
    pub operation_timeout_secs: u64,
    /// Max batch size for flush operations
    pub max_batch_size: usize,
}

impl Default for UsageEngineConfig {
    fn default() -> Self {
        Self {
            counted_ttl_secs: 3600,                  // 1 hour
            monthly_ttl_secs: 35 * 24 * 60 * 60,    // 35 days
            hourly_ttl_secs: 7200,                   // 2 hours
            operation_timeout_secs: 5,
            max_batch_size: 100,
        }
    }
}

/// Configuration for client-level usage engine
#[derive(Debug, Clone)]
pub struct ClientUsageEngineConfig {
    /// TTL for counted keys (prevents replay, e.g., 1 hour)
    pub counted_ttl_secs: i64,
    /// TTL for hourly metric keys (2 hours)
    pub hourly_ttl_secs: i64,
    /// Redis operation timeout
    pub operation_timeout_secs: u64,
}

impl Default for ClientUsageEngineConfig {
    fn default() -> Self {
        Self {
            counted_ttl_secs: 3600,    // 1 hour
            hourly_ttl_secs: 7200,     // 2 hours
            operation_timeout_secs: 5,
        }
    }
}
