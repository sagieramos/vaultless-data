use axum::{
    Extension,
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use vaultless_core::{ApiKey, MetricsConfig, SubscriptionTier, increment_rate_limit_hit_pool};

use crate::{middleware::error::ApiError, services::RateLimiter, state::AppState};

/// Default rate limit when subscription tier is unknown.
/// Uses the Free tier default (60 requests/minute).
const DEFAULT_RATE_LIMIT: i32 = 60;

/// Rate limiting middleware for API key
pub async fn rate_limit_by_api_key(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let rate_limiter = RateLimiter::new(state.redis_pool.clone());

    // Check rate limit using default (subscription-based limits would need subscription lookup)
    let result = rate_limiter
        .check_api_key_limit(api_key.id, DEFAULT_RATE_LIMIT)
        .await?;

    if !result.allowed {
        // Record violation
        increment_rate_limit_hit_pool(&state.redis_pool, api_key.id, &MetricsConfig::default())
            .await?;
        if let Err(e) = rate_limiter.record_violation(api_key.id).await {
            tracing::warn!("Failed to record rate limit violation: {}", e);
        }

        // Record in usage metrics
        if let Err(e) =
            increment_rate_limit_hit_pool(&state.redis_pool, api_key.id, &MetricsConfig::default())
                .await
        {
            tracing::warn!("Failed to record rate limit hit in metrics: {}", e);
        }

        tracing::warn!(
            api_key_id = %api_key.id,
            limit = result.limit,
            "Rate limit exceeded"
        );

        return Err(rate_limit_error(result));
    }

    // Add rate limit headers to response
    let mut response = next.run(request).await;
    add_rate_limit_headers(&mut response, &result);

    Ok(response)
}

/// Rate limiting middleware by IP (for unauthenticated endpoints)
pub async fn rate_limit_by_ip(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let rate_limiter = RateLimiter::new(state.redis_pool.clone());
    let ip = addr.ip().to_string();

    // Default IP limit: 100 requests per minute
    let ip_limit = 100;

    let result = rate_limiter.check_ip_limit(&ip, ip_limit).await?;

    if !result.allowed {
        tracing::warn!(
            ip = %ip,
            limit = result.limit,
            "IP rate limit exceeded"
        );

        return Err(rate_limit_error(result));
    }

    let mut response = next.run(request).await;
    add_rate_limit_headers(&mut response, &result);

    Ok(response)
}

/// Endpoint-specific rate limiting (e.g., send message has stricter limit)
pub async fn rate_limit_endpoint(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let rate_limiter = RateLimiter::new(state.redis_pool.clone());
    let endpoint = request.uri().path();

    // Endpoint-specific limits (based on default rate limit)
    let base_limit = DEFAULT_RATE_LIMIT as f64;
    let (limit, window) = match endpoint {
        path if path.contains("/messages/send") => {
            // Stricter limit for sending messages
            let limit = (base_limit * 0.5) as i64; // 50% of global limit
            (limit, 60)
        }
        path if path.contains("/analytics") => {
            // Analytics can be more relaxed
            let limit = (base_limit * 1.5) as i64; // 150% of global limit
            (limit, 60)
        }
        _ => {
            // Use default global limit
            return Ok(next.run(request).await);
        }
    };

    let result = rate_limiter
        .check_endpoint_limit(api_key.id, endpoint, limit, window)
        .await?;

    if !result.allowed {
        tracing::warn!(
            api_key_id = %api_key.id,
            endpoint = %endpoint,
            limit = result.limit,
            "Endpoint rate limit exceeded"
        );

        return Err(rate_limit_error(result));
    }

    let mut response = next.run(request).await;
    add_rate_limit_headers(&mut response, &result);

    Ok(response)
}

/// Add rate limit headers to response
fn add_rate_limit_headers(
    response: &mut Response,
    result: &crate::services::rate_limiter::RateLimitResult,
) {
    let headers = response.headers_mut();

    // Standard rate limit headers
    headers.insert(
        "X-RateLimit-Limit",
        result.limit.to_string().parse().unwrap(),
    );

    headers.insert(
        "X-RateLimit-Remaining",
        result.remaining.to_string().parse().unwrap(),
    );

    headers.insert(
        "X-RateLimit-Reset",
        result.reset_at.to_string().parse().unwrap(),
    );

    if let Some(retry_after) = result.retry_after {
        headers.insert("Retry-After", retry_after.to_string().parse().unwrap());
    }
}

/// Create rate limit error response
fn rate_limit_error(result: crate::services::rate_limiter::RateLimitResult) -> ApiError {
    let retry_after = result.retry_after.unwrap_or(60);

    ApiError::too_many_requests(format!(
        "Rate limit exceeded. Limit: {} requests per minute. Try again in {} seconds.",
        result.limit, retry_after
    ))
    .with_code("RATE_LIMIT_EXCEEDED")
}

/// Tiered rate limiting based on subscription
pub struct TieredRateLimitConfig {
    pub requests_per_minute: i32,
    pub burst_allowance: i32,
}

impl TieredRateLimitConfig {
    /// Create rate limit config from subscription tier
    pub fn from_tier(tier: SubscriptionTier) -> Self {
        let base_limit = tier.default_rate_limit();

        // Allow burst of 20% above limit for short periods
        let burst_allowance = (base_limit as f64 * 1.2) as i32;

        Self {
            requests_per_minute: base_limit,
            burst_allowance,
        }
    }

    /// Create rate limit config with default (Free tier) limits
    pub fn default_config() -> Self {
        Self::from_tier(SubscriptionTier::Free)
    }
}
