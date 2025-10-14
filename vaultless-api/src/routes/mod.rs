pub mod health;

use axum::{Router, routing::get};

use crate::state::AppState;

/// Build the main application router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health check routes (no auth required)
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check))
        .route("/live", get(health::liveness_check))
        .route("/check_cache", get(health::check_cache_handler))
        // TODO: Add authenticated API routes here
        // .nest("/api/v1", api_routes())
        .with_state(state)
}

// Future: API v1 routes
// fn api_routes() -> Router<AppState> {
//     Router::new()
//         .route("/messages/send", post(handlers::send_message))
//         .route("/messages/:recipient_id", get(handlers::receive_messages))
//         .layer(axum::middleware::from_fn_with_state(state, require_auth))
// }
