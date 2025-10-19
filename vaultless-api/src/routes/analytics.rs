use crate::{
    handlers::analytics::*,
    middleware::{api_key_auth::require_client_api_key, token_auth::require_user_auth},
    state::AppState,
};
use axum::{
    Router,
    routing::{get, post},
};

pub fn analytics_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Analytics Endpoints (require both user session and API key ownership)
        .route("/analytics/dashboard", get(get_dashboard))
        .route("/analytics/usage/timeseries", get(get_usage_timeseries))
        .route("/analytics/quota/status", get(get_quota_status))
        .route("/analytics/costs", get(get_cost_breakdown))
        .route("/analytics/export", post(export_analytics))
        .route("/analytics/trends", get(get_usage_trends))
        .route("/analytics/overview", get(get_usage_overview))
        .route("/analytics/tier", get(get_tier_info))
        // Enforce both: session user (Authorization header) + valid API key (X-Api-Key or Bearer)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            require_client_api_key,
        ))
}
