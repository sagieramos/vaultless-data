pub mod analytics;
pub mod client;
pub mod health;
pub mod instant_message;
pub mod message;

//pub mod proof;
pub mod user_auth;
use axum::{Router, routing::get};

use user_auth::auth_routes;

use crate::state::AppState;

/// Builds the complete API router with nested sub-routes for modularity.
pub fn build_routes(state: AppState) -> Router {
    Router::new()
        // Public health check endpoint.
        .route("/health", get(health::health_check))
        // Nested auth routes (public and protected).
        .nest("/auth", auth_routes(state.clone()))
        .nest("/api/clients", client::client_routes())
        .nest("/api/messages", instant_message::message_routes())
        .with_state(state)
}
