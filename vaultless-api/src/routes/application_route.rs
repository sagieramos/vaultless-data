use axum::{
    Router, middleware,
    routing::{delete, get, patch, post},
};

use crate::{
    handlers::developer::{analytics, application},
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
        .route(
            "/{id}/monthly-revenue",
            get(analytics::get_application_monthly_revenue),
        )
        .route(
            "/monthly-revenue",
            get(analytics::get_developer_monthly_revenue),
        )
        .route(
            "/monthly-breakdown",
            get(analytics::get_monthly_revenue_breakdown),
        )
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    // Routes without query parameters that read from materialized view (ETag cacheable)
    let mv_cached_no_query_routes = Router::new()
        .route(
            "/{id}/analytics",
            get(application::get_application_analytics),
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
            "/{id}/upgrade-recommendations",
            get(analytics::get_application_upgrade_recommendations),
        )
        // Key rotation routes
        .route(
            "/{id}/keys/secret/rotate",
            post(application::rotate_secret_key),
        )
        .route(
            "/{id}/keys/publishable/rotate",
            post(application::rotate_publishable_key),
        )
        .route(
            "/{id}/keys/publishable",
            post(application::add_publishable_key),
        )
        .route(
            "/{id}/keys/publishable/deactivate",
            post(application::deactivate_publishable_key),
        )
        .layer(middleware::from_fn(reject_all_query))
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    Router::new()
        .merge(mv_cached_query_routes)
        .merge(non_cached_query_routes)
        .merge(mv_cached_no_query_routes)
        .merge(non_cached_no_query_routes)
}
