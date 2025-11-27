use axum::{
    Router, middleware,
    routing::{delete, get, patch, post, put},
};

use crate::{
    handlers::application,
    middleware::{
        application::validate_uuid_and_check_ownership, global::reject_all_query,
        user::require_user_auth,
    },
    state::{self, AppState},
};

pub fn application_routes(state: AppState) -> Router<AppState> {
    let id_routes = Router::new()
        .route("/{id}", patch(application::update_application))
        .route("/{id}", delete(application::deactivate_application))
        .route(
            "/{id}/health",
            get(application::get_application_health),
        )
        .route(
            "/{id}/tier",
            put(application::update_application_tier),
        )
        .route(
            "/{id}/stats",
            get(application::get_application_stats),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            validate_uuid_and_check_ownership,
        ));

    Router::new()
        .route("/", post(application::create_application))
        .route("/", get(application::list_applications))
        .route("/{id}", get(application::get_application))
        .merge(id_routes) // merge routes with ID checks
        .layer(middleware::from_fn(reject_all_query))
        .layer(middleware::from_fn_with_state(state, require_user_auth))
}
