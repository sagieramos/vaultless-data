//! # Usage Metrics Module
//!
//! Provides efficient Redis-based aggregation and periodic flushing of API usage metrics
//! into Postgres for durable storage. Designed for 20k+ RPS workloads with Redis atomic
//! operations and batched upserts.
//!
//! ## Module Structure
//!
//! - `config` - Configuration, constants, and type aliases
//! - `counters` - Metric counter types and Redis key management
//! - `increment` - Redis increment operations (hot path)
//! - `flusher` - Background flusher for Redis to Postgres persistence
//! - `queries` - Postgres aggregate queries
//! - `aggregate` - TimescaleDB continuous aggregate queries
//! - `restoration` - Redis state restoration from Postgres

pub mod aggregate;
pub mod config;
pub mod counters;
pub mod flusher;
pub mod increment;
pub mod queries;
pub mod restoration;

// =============================================================================
// Re-exports
// =============================================================================

// Config
pub use config::{
    MetricsConfig, RedisPoolType, ACTIVE_KEYS_SET, REDIS_OPERATION_TIMEOUT_SECS,
};

// Counters
pub use counters::{
    FlusherMetrics, MetricCounters, MetricGranularity, MetricKey,
    get_hour_window, get_minute_window,
};

// Increment operations
pub use increment::{
    increment_message_received_pool, increment_message_sent_pool,
    increment_proof_verified_pool, increment_rate_limit_hit_pool,
};

// Flusher
pub use flusher::start_redis_flusher;

// Queries
pub use queries::{UsageAggregate, get_aggregate_by_application_id};

// Aggregate queries
pub use aggregate::{
    DailyUsageSummary, MonthlyTotal, UsageTrends,
    get_realtime_usage, get_usage_trends,
};
