use crate::middleware::error::ApiError;
use axum::{body::Body, http::Request, middleware::Next, response::Response};
use once_cell::sync::Lazy;
use regex::Regex;
use tracing::warn;

/// Middleware that rejects requests containing forbidden query parameters
pub async fn reject_all_query(req: Request<Body>, next: Next) -> Result<Response, ApiError> {
    if req.uri().query().is_some() {
        let query = req.uri().query().unwrap_or_default().to_string();

        warn!(
            "Rejected request with unexpected query parameters: {}",
            query
        );

        return Err(
            ApiError::bad_request(format!("Unexpected query parameters found: {}", query))
                .with_code("UNEXPECTED_QUERY_PARAMS"),
        );
    }

    Ok(next.run(req).await)
}

// ---

/// Middleware that rejects suspicious or forbidden query parameters.
///
/// This helps prevent SQL injection, malformed queries,
/// and unauthorized attempts.
///

static SUSPICIOUS_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"(?i)\b(drop|delete|update|insert|alter|truncate|grant|revoke)\b").unwrap(),
        Regex::new(r"--").unwrap(),
        Regex::new(r";").unwrap(),
        Regex::new(r"(?i)union\s+select").unwrap(),
        Regex::new(r"(?i)exec\s").unwrap(),
        Regex::new(r"(?i)sleep\(").unwrap(),
        Regex::new(r"\$\w+").unwrap(),
    ]
});

pub async fn reject_suspicious_query(req: Request<Body>, next: Next) -> Result<Response, ApiError> {
    if let Some(query) = req.uri().query() {
        for pattern in SUSPICIOUS_PATTERNS.iter() {
            if pattern.is_match(query) {
                warn!("Rejected suspicious query: {}", query);

                return Err(ApiError::forbidden(
                    "Request blocked due to suspicious query content.",
                )
                .with_code("SUSPICIOUS_QUERY_BLOCKED"));
            }
        }
    }

    Ok(next.run(req).await)
}
