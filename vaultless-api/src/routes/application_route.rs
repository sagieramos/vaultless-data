use axum::{
    Router, middleware,
    routing::{delete, get, patch, post},
};

use crate::{
    handlers::developer::{analytics, application},
    middleware::{global::reject_all_query, user::user_auth},
    state::AppState,
};

pub fn application_routes(state: AppState) -> Router<AppState> {
    // Routes that need query parameters
    let query_routes = Router::new()
        .route("/{id}/chart", get(application::get_chart_data))
        .route("/{id}/export", get(analytics::export_application_usage))
        .route("/quota-warnings", get(application::get_quota_warnings))
        .route("/", get(application::list_applications)) // has pagination
        .layer(middleware::from_fn_with_state(state.clone(), user_auth));

    // Routes without query parameters
    let no_query_routes = Router::new()
        .route("/", post(application::create_application))
        .route("/{id}", get(application::get_application))
        .route("/{id}", patch(application::update_application))
        .route("/{id}", delete(application::deactivate_application))
        .route(
            "/{id}/with_keys",
            get(application::get_application_with_keys_handler),
        )
        .route("/usage-summary", get(application::get_user_usage_summary))
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

    Router::new().merge(query_routes).merge(no_query_routes)
}
