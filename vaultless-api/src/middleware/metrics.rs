// api/src/middleware/metrics.rs
use axum::{extract::Request, middleware::Next, response::Response};
use prometheus::{HistogramOpts, HistogramVec, IntCounterVec, Opts};
use std::time::Instant;

use once_cell::sync::Lazy;

//----------------------------------------------------
// Global Prometheus metrics
//----------------------------------------------------
pub static HTTP_REQUESTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let counter = IntCounterVec::new(
        Opts::new("http_requests_total", "Number of HTTP requests received"),
        &["method", "path", "status"],
    )
    .expect("Failed to create HTTP_REQUESTS_TOTAL metric");

    prometheus::register(Box::new(counter.clone()))
        .expect("Failed to register HTTP_REQUESTS_TOTAL");

    counter
});

pub static HTTP_REQUEST_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let histogram = HistogramVec::new(
        HistogramOpts::new(
            "http_request_duration_seconds",
            "HTTP request duration in seconds",
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        &["method", "path"],
    )
    .expect("Failed to create HTTP_REQUEST_DURATION_SECONDS metric");

    prometheus::register(Box::new(histogram.clone()))
        .expect("Failed to register HTTP_REQUEST_DURATION_SECONDS");

    histogram
});

pub static DB_QUERY_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    let histogram = HistogramVec::new(
        HistogramOpts::new(
            "db_query_duration_seconds",
            "Database query duration in seconds",
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
        &["query_type"],
    )
    .expect("Failed to create DB_QUERY_DURATION_SECONDS metric");

    prometheus::register(Box::new(histogram.clone()))
        .expect("Failed to register DB_QUERY_DURATION_SECONDS");

    histogram
});

pub static CACHE_OPERATIONS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    let counter = IntCounterVec::new(
        Opts::new("cache_operations_total", "Number of cache operations"),
        &["operation", "status"],
    )
    .expect("Failed to create CACHE_OPERATIONS_TOTAL metric");

    prometheus::register(Box::new(counter.clone()))
        .expect("Failed to register CACHE_OPERATIONS_TOTAL");

    counter
});

/// Normalize paths to avoid high cardinality in metrics
/// Examples:
/// - /users/123 -> /users/:id
/// - /api/v1/posts/456/comments -> /api/v1/posts/:id/comments
/// - /users/550e8400-e29b-41d4-a716-446655440000 -> /users/:id
fn normalize_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let normalized: Vec<String> = segments
        .iter()
        .map(|segment| {
            // Skip empty segments
            if segment.is_empty() {
                return segment.to_string();
            }

            // Check if segment looks like an ID
            if is_id_like(segment) {
                ":id".to_string()
            } else {
                segment.to_string()
            }
        })
        .collect();

    normalized.join("/")
}

/// Check if a string looks like an ID
fn is_id_like(s: &str) -> bool {
    // Numeric ID
    if s.parse::<i64>().is_ok() {
        return true;
    }

    // UUID (standard format or without hyphens)
    if s.len() == 36 || s.len() == 32 {
        let clean = s.replace('-', "");
        if clean.len() == 32 && clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }

    // Long alphanumeric strings (likely IDs)
    if s.len() > 20 && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return true;
    }

    false
}

/// Middleware to track HTTP request metrics
pub async fn track_metrics(req: Request, next: Next) -> Response {
    // Force lazy initialization of metrics
    Lazy::force(&HTTP_REQUESTS_TOTAL);
    Lazy::force(&HTTP_REQUEST_DURATION_SECONDS);
    Lazy::force(&DB_QUERY_DURATION_SECONDS);
    Lazy::force(&CACHE_OPERATIONS_TOTAL);

    let start = Instant::now();
    let method = req.method().to_string();
    let raw_path = req.uri().path().to_string();
    let normalized_path = normalize_path(&raw_path);

    // Call the next handler
    let response = next.run(req).await;

    // Record metrics after request completes
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    // Track request count
    HTTP_REQUESTS_TOTAL
        .with_label_values(&[&method, &normalized_path, &status])
        .inc();

    // Track request duration
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[&method, &normalized_path])
        .observe(duration);

    tracing::debug!(
        method = %method,
        path = %normalized_path,
        status = %status,
        duration_ms = %format!("{:.2}", duration * 1000.0),
        "Request completed"
    );

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/users/123"), "/users/:id");
        assert_eq!(
            normalize_path("/api/v1/posts/456/comments"),
            "/api/v1/posts/:id/comments"
        );
        assert_eq!(normalize_path("/health"), "/health");
        assert_eq!(normalize_path("/metrics"), "/metrics");
        assert_eq!(
            normalize_path("/users/550e8400-e29b-41d4-a716-446655440000"),
            "/users/:id"
        );
        assert_eq!(
            normalize_path("/items/abc123def456ghi789jkl"),
            "/items/:id"
        );
        assert_eq!(normalize_path("/api/status"), "/api/status");
    }

    #[test]
    fn test_is_id_like() {
        // Numeric IDs
        assert!(is_id_like("123"));
        assert!(is_id_like("9876543210"));

        // UUIDs
        assert!(is_id_like("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_id_like("550e8400e29b41d4a716446655440000"));

        // Long alphanumeric
        assert!(is_id_like("abc123def456ghi789jkl"));

        // Not IDs
        assert!(!is_id_like("users"));
        assert!(!is_id_like("api"));
        assert!(!is_id_like("v1"));
        assert!(!is_id_like("short"));
    }
}