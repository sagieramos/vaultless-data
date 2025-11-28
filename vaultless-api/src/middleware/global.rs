use crate::middleware::error::ApiError;
use axum::{body::Body, http::Request, middleware::Next, response::Response};
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
pub async fn reject_suspicious_query(req: Request<Body>, next: Next) -> Result<Response, ApiError> {
    let query_opt = req.uri().query();

    if let Some(query) = query_opt {
        // List of suspicious patterns...
        let suspicious_patterns = [
            r"(?i)\b(drop|delete|update|insert|alter|truncate|grant|revoke)\b",
            r"(?i)--",
            r"(?i);",
            r"(?i)union\s+select",
            r"(?i)exec\s",
            r"(?i)sleep\(",
            r"(?i)\$\w+",
        ];

        for pattern in suspicious_patterns {
            let re = match Regex::new(pattern) {
                // If a pattern is invalid (internal error), return 500
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Internal regex pattern error: {}", e);
                    // This is an internal server issue, so 500 is appropriate here.
                    return Err(ApiError::internal_server_error(
                        "Failed to process security checks",
                    ));
                }
            };

            if re.is_match(query) {
                warn!(
                    "Rejected suspicious query: {} | pattern matched: {}",
                    query, pattern
                );

                return Err(ApiError::forbidden(
                    "Request blocked due to suspicious query content.",
                )
                .with_code("SUSPICIOUS_QUERY_BLOCKED"));
            }
        }
    }

    Ok(next.run(req).await)
}
