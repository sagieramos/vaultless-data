pub mod application;
pub mod client;
pub mod config;
pub mod counters;

// Re-export commonly used types
pub use config::MetricsConfig;
pub use counters::{MetricCounters, FlusherMetrics};
pub use application::flusher::start_redis_flusher;

// Re-export active clients functions
pub use client::{get_active_clients_count, get_active_client_ids};