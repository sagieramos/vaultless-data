use axum::{
    Router, middleware,
    routing::{delete, get, patch, post},
};

use crate::{
    handlers::developer::{analytics, application, application_dashboard},
    middleware::{etag::mv_etag_middleware, global::reject_all_query, user::user_auth},
    state::AppState,
};

pub fn application_routes(state: AppState) -> Router<AppState> {
    // Routes that read from materialized view and support ETag caching
    // These routes benefit from 304 Not Modified responses when data hasn't changed
    let mv_cached_query_routes = Router::new()
        .route("/quota-warnings", get(application::get_quota_warnings))
        .route("/", get(application::list_applications)) // has pagination
        .layer(middleware::from_fn_with_state(state.clone(), mv_etag_middleware))
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    // Routes with query params but no MV caching (data changes frequently or is real-time)
    let non_cached_query_routes = Router::new()
        .route("/{id}/chart", get(application::get_chart_data))
        .route("/{id}/export", get(analytics::export_application_usage))
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    // Routes without query parameters that read from materialized view (ETag cacheable)
    let mv_cached_no_query_routes = Router::new()
        .route(
            "/{id}/with_keys",
            get(application::get_application_with_keys_handler),
        )
        .route(
            "/{id}/analytics",
            get(application_dashboard::get_application_with_keys_handler),
        )
        .route("/usage-summary", get(application::get_user_usage_summary))
        .layer(middleware::from_fn_with_state(state.clone(), mv_etag_middleware))
        .layer(middleware::from_fn(reject_all_query))
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    // Routes without query parameters that don't use MV or need real-time data
    let non_cached_no_query_routes = Router::new()
        .route("/", post(application::create_application))
        .route("/{id}", patch(application::update_application))
        .route("/{id}", delete(application::deactivate_application))
        .route(
            "/{id}/quota-status",
            get(analytics::get_application_quota_status),
        )
        .route(
            "/{id}/costs",
            get(analytics::get_application_cost_breakdown),
        )
        .route("/{id}/trends", get(analytics::get_application_trends))
        .layer(middleware::from_fn(reject_all_query))
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    Router::new()
        .merge(mv_cached_query_routes)
        .merge(non_cached_query_routes)
        .merge(mv_cached_no_query_routes)
        .merge(non_cached_no_query_routes)
}
