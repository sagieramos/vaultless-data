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
//! - `engine` - Atomic Lua scripts for billing-critical operations
//! - `increment` - Redis increment operations (legacy hot path)
//! - `flusher` - Background flusher for Redis to Postgres persistence
//! - `queries` - Postgres aggregate queries
//! - `aggregate` - TimescaleDB continuous aggregate queries
//! - `restoration` - Redis state restoration from Postgres

pub mod aggregate;
pub mod config;
pub mod counters;
pub mod engine;
pub mod flusher;
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

// Flusher
pub use flusher::start_redis_flusher;

// Queries
pub use queries::{UsageAggregate, get_aggregate_by_application_id};

// Aggregate queries
pub use aggregate::{
    DailyUsageSummary, MonthlyTotal, UsageTrends,
    get_realtime_usage, get_usage_trends,
};

// Engine - Atomic Lua scripts for billing-critical operations
pub use engine::{
    get_session_counters, increment_session_counters, record_message_events,
    record_message_received, record_message_sent, record_proof_verified,
    record_rate_limit_hit, session_metric_key, EngineConfig,
    RecordMessageReceivedInput, RecordMessageSentInput, RecordProofVerifiedInput,
    RecordRateLimitHitInput,
};
