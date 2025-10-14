use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

/// Request logging middleware
pub async fn log_request(request: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();

    // Get request ID from headers or generate one
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Create span for this request
    let span = tracing::info_span!(
        "request",
        method = %method,
        uri = %uri,
        version = ?version,
        request_id = %request_id,
    );

    let _guard = span.enter();

    tracing::info!("Request started");

    // Process request
    let response = next.run(request).await;

    // Log response
    let status = response.status();
    let duration = start.elapsed();

    let level = if status.is_server_error() {
        tracing::Level::ERROR
    } else if status.is_client_error() {
        tracing::Level::WARN
    } else {
        tracing::Level::INFO
    };

    match level {
        tracing::Level::ERROR => {
            tracing::error!(
                status = %status,
                duration_micros_s = %duration.as_micros(),
                "Request completed"
            );
        }
        tracing::Level::WARN => {
            tracing::warn!(
                status = %status,
                duration_micros_s = %duration.as_micros(),
                "Request completed"
            );
        }
        _ => {
            tracing::info!(
                status = %status,
                duration_micros_s = %duration.as_micros(),
                "Request completed"
            );
        }
    }

    response
}

/// Add request ID to response headers
pub async fn add_request_id(mut request: Request, next: Next) -> Response {
    // Generate or extract request ID
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Add to request extensions
    request.extensions_mut().insert(request_id.clone());

    // Process request
    let mut response = next.run(request).await;

    // Add request ID to response headers
    response
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());

    response
}
