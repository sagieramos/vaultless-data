pub mod health;

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::{
    handlers,
    middleware::{
        rate_limit_by_api_key, rate_limit_by_ip, rate_limit_endpoint, require_token_auth,
    },
    state::AppState,
};

/// Build the main application router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health check routes (with IP rate limiting)
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check))
        .route("/live", get(health::liveness_check))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
        // API v1 routes (authenticated)
        .nest("/api/v1", api_v1_routes(state.clone()))
        .with_state(state)
}

/// API v1 routes
fn api_v1_routes(state: AppState) -> Router<AppState> {
    // Admin routes (NO AUTH - temporary for development)
    let admin_routes = Router::new()
        .route("/admin/keys/create", post(handlers::create_api_key))
        .route("/admin/keys", get(handlers::list_api_keys))
        .route(
            "/admin/keys/{key_id}/rate-limit",
            get(handlers::get_rate_limit_status),
        )
        .route(
            "/admin/keys/{key_id}/rate-limit/reset",
            post(handlers::reset_rate_limit),
        )
        .layer(from_fn_with_state(state.clone(), rate_limit_by_ip));

    // Message routes (AUTH + RATE LIMIT)
    let message_routes = Router::new()
        .route("/messages/send", post(handlers::send_message))
        .route("/messages/{recipient_id}", get(handlers::receive_messages))
        .route(
            "/messages/{message_id}/metadata",
            get(handlers::get_message_metadata),
        )
        // Apply endpoint-specific rate limiting first
        .layer(from_fn_with_state(state.clone(), rate_limit_endpoint))
        // Then apply global API key rate limiting
        .layer(from_fn_with_state(state.clone(), rate_limit_by_api_key))
        // Finally apply authentication
        .layer(from_fn_with_state(state.clone(), require_token_auth));

    // Analytics routes (AUTH + RATE LIMIT)
    let analytics_routes = Router::new()
        .route("/analytics/dashboard", get(handlers::get_dashboard))
        .route("/analytics/daily", get(handlers::get_daily_usage))
        .route("/analytics/weekly", get(handlers::get_weekly_usage))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_api_key))
        .layer(from_fn_with_state(state.clone(), require_token_auth));

    // Rate limit monitoring routes (AUTH)
    let rate_limit_routes = Router::new()
        .route(
            "/rate-limit/status",
            get(handlers::get_my_rate_limit_status),
        )
        .route("/rate-limit/history", get(handlers::get_rate_limit_history))
        .layer(from_fn_with_state(state.clone(), require_token_auth));

    // Combine routes
    admin_routes
        .merge(message_routes)
        .merge(analytics_routes)
        .merge(rate_limit_routes)
}
