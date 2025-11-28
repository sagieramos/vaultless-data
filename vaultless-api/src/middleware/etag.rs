use crate::state::AppState;
use axum::http::HeaderMap;
use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use hyper::{StatusCode, header};
use std::sync::Arc;
use vaultless_core::get_global_mv_etag;

/// Middleware that adds ETag support for materialized view endpoints
///
/// This middleware:
/// 1. Checks Redis for a canonical ETag timestamp
/// 2. Compares with client's If-None-Match header
/// 3. Returns 304 Not Modified if ETags match
/// 4. Adds ETag and Cache-Control headers to responses
///
/// # Usage
/// ```rust
/// Router::new()
///     .route("/api/v1/applications", get(list_applications))
///     .layer(middleware::from_fn_with_state(
///         state.clone(),
///         mv_etag_middleware
///     ))
/// ```
pub async fn mv_etag_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let redis_pool = Arc::clone(&state.redis_pool);
    let maybe_ts = match get_global_mv_etag(&redis_pool).await {
        Ok(Some(ts)) => Some(ts),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!("Redis unavailable for MV ETag: {}", err);
            None
        }
    };

    let etag = if let Some(ts) = maybe_ts {
        format!("W/\"{}\"", ts)
    } else {
        format!("W/\"{}\"", Utc::now().timestamp_millis())
    };

    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH) {
        if let Ok(client_etag) = if_none_match.to_str() {
            if client_etag == etag {
                return Ok((StatusCode::NOT_MODIFIED, Body::empty()).into_response());
            }
        }
    }

    let mut response = next.run(request).await;

    if response.status().is_success() {
        response.headers_mut().insert(
            header::ETAG,
            header::HeaderValue::from_str(&etag)
                .unwrap_or_else(|_| header::HeaderValue::from_static("W/\"0\"")),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("public, max-age=60"),
        );
    }

    Ok(response)
}

/// Configurable ETag middleware with custom cache duration
#[derive(Debug, Clone)]
pub struct MvEtagConfig {
    pub max_age_seconds: u32,
    pub stale_while_revalidate: Option<u32>,
    /// Whether to use response body for fallback ETag calculation
    pub use_body_fallback: bool,
}

impl Default for MvEtagConfig {
    fn default() -> Self {
        Self {
            max_age_seconds: 60,
            stale_while_revalidate: Some(300), // 5 minutes
            use_body_fallback: false,          // Disabled by default for performance
        }
    }
}

/// Middleware builder pattern for more flexibility
pub struct MvEtagMiddleware {
    config: MvEtagConfig,
}

impl MvEtagMiddleware {
    pub fn new() -> Self {
        Self {
            config: MvEtagConfig::default(),
        }
    }

    pub fn with_max_age(mut self, seconds: u32) -> Self {
        self.config.max_age_seconds = seconds;
        self
    }

    pub fn with_stale_while_revalidate(mut self, seconds: u32) -> Self {
        self.config.stale_while_revalidate = Some(seconds);
        self
    }

    pub fn with_body_fallback(mut self, enabled: bool) -> Self {
        self.config.use_body_fallback = enabled;
        self
    }

    pub fn build(self) -> MvEtagConfig {
        self.config
    }
}

pub async fn mv_etag_middleware_with_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let config = MvEtagConfig::default();
    mv_etag_middleware_configurable(state, headers, request, next, config).await
}

async fn mv_etag_middleware_configurable(
    state: AppState,
    headers: HeaderMap,
    request: Request,
    next: Next,
    config: MvEtagConfig,
) -> Result<Response, StatusCode> {
    let redis_pool = Arc::clone(&state.redis_pool);
    let maybe_ts = match get_global_mv_etag(&redis_pool).await {
        Ok(Some(ts)) => Some(ts),
        Ok(None) => {
            tracing::debug!("No MV ETag found in Redis");
            None
        }
        Err(err) => {
            tracing::warn!("Redis unavailable for MV ETag: {}", err);
            None
        }
    };

    let etag = if let Some(ts) = maybe_ts {
        format!("W/\"{}\"", ts)
    } else {
        // Fallback: use current timestamp
        // Note: This means clients might get 304 responses even when data changed
        // if Redis is temporarily unavailable. This is a trade-off for availability.
        format!("W/\"{}\"", Utc::now().timestamp_millis())
    };

    if let Some(if_none_match) = headers.get(header::IF_NONE_MATCH) {
        if let Ok(client_etag) = if_none_match.to_str() {
            if client_etag == etag {
                tracing::debug!("ETag match, returning 304 Not Modified");
                return Ok((StatusCode::NOT_MODIFIED, Body::empty()).into_response());
            }
        }
    }

    let mut response = next.run(request).await;

    if response.status().is_success() {
        response.headers_mut().insert(
            header::ETAG,
            header::HeaderValue::from_str(&etag)
                .unwrap_or_else(|_| header::HeaderValue::from_static("W/\"0\"")),
        );

        let cache_control = if let Some(swr) = config.stale_while_revalidate {
            format!(
                "public, max-age={}, stale-while-revalidate={}",
                config.max_age_seconds, swr
            )
        } else {
            format!("public, max-age={}", config.max_age_seconds)
        };

        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_str(&cache_control)
                .unwrap_or_else(|_| header::HeaderValue::from_static("public, max-age=60")),
        );

        // Optional: Add Vary header to indicate ETag varies by user
        response.headers_mut().insert(
            header::VARY,
            header::HeaderValue::from_static("Cookie, Authorization"),
        );
    }

    Ok(response)
}

// src/routes/mod.rs (router setup)
/*
use crate::{middleware::etag::mv_etag_middleware, state::AppState};
use axum::{Router, middleware, routing::get};

pub fn application_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/applications", get(list_applications))
        .route("/applications/usage-summary", get(get_user_usage_summary))
        .route("/applications/quota-warnings", get(get_quota_warnings))
        // Apply middleware to all routes in this group
        .layer(middleware::from_fn_with_state(
            state.clone(),
            mv_etag_middleware,
        ))
}

// Alternative: Apply selectively to specific routes
pub fn application_routes_selective(state: AppState) -> Router<AppState> {
    let etag_layer = middleware::from_fn_with_state(state.clone(), mv_etag_middleware);

    Router::new()
        .route("/applications", get(list_applications))
        .route("/applications/usage-summary", get(get_user_usage_summary))
        .layer(etag_layer.clone()) // Apply to these routes
        .route("/applications/:id/create-key", post(create_key))
    // This route doesn't get ETag middleware
}
 */
