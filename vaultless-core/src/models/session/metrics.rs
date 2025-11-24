use prometheus::{
    register_histogram, register_int_counter, register_int_gauge, Histogram, IntCounter, IntGauge,
};
use once_cell::sync::Lazy;

pub static CACHE_HITS: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_cache_hits_total",
        "Total number of local cache hits"
    )
    .unwrap()
});

pub static CACHE_MISSES: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_cache_misses_total",
        "Total number of local cache misses"
    )
    .unwrap()
});

pub static REDIS_CHECKS: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_redis_checks_total",
        "Total number of Redis revocation checks"
    )
    .unwrap()
});

pub static INVALIDATIONS_RECEIVED: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_invalidations_received_total",
        "Total number of invalidation messages received via pub/sub"
    )
    .unwrap()
});

pub static INVALIDATIONS_SENT: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_invalidations_sent_total",
        "Total number of invalidation messages published"
    )
    .unwrap()
});

pub static PUBSUB_HEALTHY: Lazy<IntGauge> = Lazy::new(|| {
    register_int_gauge!(
        "session_pubsub_healthy",
        "Pub/sub connection health status (1=healthy, 0=unhealthy)"
    )
    .unwrap()
});

pub static PUBSUB_RECONNECTS: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_pubsub_reconnects_total",
        "Total number of pub/sub reconnection attempts"
    )
    .unwrap()
});

pub static VERIFY_DURATION: Lazy<Histogram> = Lazy::new(|| {
    register_histogram!(
        "session_verify_duration_seconds",
        "Duration of session verification operations",
        vec![0.00001, 0.00005, 0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05]
    )
    .unwrap()
});

pub static FALLBACK_TO_REDIS: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_fallback_to_redis_total",
        "Total number of fallbacks to Redis-only verification"
    )
    .unwrap()
});

pub static STALE_CACHE_DETECTED: Lazy<IntCounter> = Lazy::new(|| {
    register_int_counter!(
        "session_stale_cache_detected_total",
        "Total number of times stale cache was detected and corrected"
    )
    .unwrap()
});