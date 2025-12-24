//! # Client Usage Metrics Module
//!
//! This module is a direct parallel to the `application` usage module, but tailored
//! for tracking and billing individual clients within an application.
//!
//! ## Core Tenets
//!
//! - **Client-Centric**: All metrics are keyed by `client_id` and scoped to an `application_id`.
//! - **High-Throughput**: Leverages Redis for real-time, atomic counting to handle high event volumes.
//! - **Durable & Consistent**: A background flusher task persists Redis data to a TimescaleDB hypertable (`client_usage_metrics`).
//! - **Atomic Operations**: Lua scripts ensure that multi-counter updates (e.g., messages sent + bytes sent) are all-or-nothing.
//!
//! ## Module Structure
//!
//! - `config`: Configuration for Redis TTLs, batch sizes, and flush intervals.
//! - `counters`: Data structures for metric counters and Redis key schemas.
//! - `engine`: Atomic Lua scripts for incrementing client metrics in Redis.
//! - `flusher`: The background service for persisting Redis data to Postgres.
//! - `queries`: SQL queries for retrieving aggregated client usage data from Postgres.
//! - `aggregate`: Queries against the continuous aggregate views (e.g., `client_usage_monthly`).
//! - `restoration`: Logic to restore Redis state from Postgres on service restarts.

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
    MetricsConfig, RedisPoolType, ACTIVE_CLIENT_KEYS_SET,
    REDIS_OPERATION_TIMEOUT_SECS,
};

// Counters
pub use counters::{
    ClientFlusherMetrics, ClientMetricCounters, ClientMetricKey,
    MetricGranularity,
    get_hour_window, get_minute_window,
};

// Flusher
pub use flusher::start_client_redis_flusher;

// Queries
pub use queries::{ClientUsageAggregate, get_aggregate_by_client_id};

// Aggregate queries
pub use aggregate::{
    MonthlyUsageSummary,
    ClientMonthlyTotal,
};

// Engine - Atomic Lua scripts for billing-critical operations
pub use engine::{
    record_client_message_received, record_client_message_sent,
    record_client_proof_verified, record_client_rate_limit_hit,
    ClientEngineConfig, RecordClientMessageReceivedInput,
    RecordClientMessageSentInput, RecordClientProofVerifiedInput,
    RecordClientRateLimitHitInput,
};
