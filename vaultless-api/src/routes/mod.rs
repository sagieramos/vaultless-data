pub mod analytics;
pub mod api_keys;
pub mod client;
pub mod health;
pub mod instant_message;
pub mod message;
//pub mod proof;
pub mod user;
use analytics::analytics_routes;
use axum::{Router, routing::get};

use {user::user_routes, api_keys::api_key_routes};

use crate::state::AppState;
pub const API_V1: &str = "/api/v1";

/// Builds the complete API router with nested sub-routes for modularity.
pub fn build_routes(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .nest("/auth", user_routes())
        .nest("/api/clients", client::client_routes())
        .nest("/api/messages", instant_message::message_routes())
        .nest("/analytics", analytics_routes(state.clone()))
         .nest("/apis", api_key_routes(state.clone()))
        .with_state(state)
}
