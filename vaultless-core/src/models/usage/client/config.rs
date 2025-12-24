//! Configuration and constants for the client usage metrics system.

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

/// Set name for tracking active metric keys for clients
pub static ACTIVE_CLIENT_KEYS_SET: Lazy<String> =
    Lazy::new(|| cache_key!("metric", "client", "active_keys"));

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
