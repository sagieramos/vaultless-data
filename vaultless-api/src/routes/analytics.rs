use crate::{
    handlers::analytics::*,
    middleware::{
        rate_limit::rate_limit_endpoint,
        token_auth::{require_api_key_ownership, require_user_auth},
    },
    state::AppState,
};
use axum::{
    Router, middleware,
    routing::{get, post},
};

use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

/// Build analytics routes with `/analytics` prefix
pub fn analytics_routes(state: AppState) -> Router<AppState> {
    let rate_limit_layer = GovernorConfigBuilder::default()
        .per_second(3)
        .burst_size(6)
        .finish()
        .unwrap();

    let analytics_router = Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/usage/timeseries", get(get_usage_timeseries))
        .route("/quota/status", get(get_quota_status))
        .route("/costs", get(get_cost_breakdown))
        .route("/export", post(export_analytics))
        .route("/trends", get(get_usage_trends))
        .route("/overview", get(get_usage_overview))
        .route("/tier", get(get_tier_info))
        // -------------------------------
        // Middleware stack
        // -------------------------------
        // Global rate limiting
        .layer(GovernorLayer::new(rate_limit_layer))
        // 1️⃣ Require user token authentication
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ))
        // 2️⃣ Require API key authentication
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key_ownership,
        ))
        // 3️⃣ Endpoint-specific rate limiting (optional stricter limits)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_endpoint,
        ));
    // Nest under /analytics prefix
    Router::new().nest("/analytics", analytics_router)
}
