use crate::{
    handlers::analytics::*,
    middleware::{api_key_auth::require_client_api_key, token_auth::require_user_auth},
    state::AppState,
};
use axum::{
    Router,
    routing::{get, post},
};

/// Build analytics routes with `/analytics` prefix
pub fn analytics_routes(state: AppState) -> Router<AppState> {
    let analytics_router = Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/usage/timeseries", get(get_usage_timeseries))
        .route("/quota/status", get(get_quota_status))
        .route("/costs", get(get_cost_breakdown))
        .route("/export", post(export_analytics))
        .route("/trends", get(get_usage_trends))
        .route("/overview", get(get_usage_overview))
        .route("/tier", get(get_tier_info))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            require_client_api_key,
        ));

    // Nest under /analytics prefix
    Router::new().nest("/analytics", analytics_router)
}
