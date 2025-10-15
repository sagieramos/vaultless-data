pub mod health;

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::{handlers, middleware::auth::require_auth, state::AppState};

/// Build the main application router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health check routes (no auth required)
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check))
        .route("/live", get(health::liveness_check))
        // API v1 routes (authenticated)
        .nest("/api/v1", api_v1_routes(state.clone()))
        .with_state(state)
}

/// API v1 routes (all require authentication)
fn api_v1_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Message endpoints
        .route("/messages/send", post(handlers::send_message))
        .route("/messages/{recipient_id}", get(handlers::receive_messages))
        .route(
            "/messages/{message_id}/metadata",
            get(handlers::get_message_metadata),
        )
        // Apply authentication middleware to all routes
        .layer(from_fn_with_state(state, require_auth))
}
