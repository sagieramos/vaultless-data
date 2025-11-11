pub mod analytics;
// pub mod api_keys;
pub mod application_route;
pub mod client;
pub mod health;
pub mod instant_message;
//pub mod proof;
pub mod user;
use analytics::analytics_routes;
use axum::{Router, middleware, routing::get};

use application_route::application_routes;
use user::user_routes;

use crate::{middleware::global::reject_suspicious_query, state::AppState};
pub const API_V1: &str = "/api/v1";

/// Builds the complete API router with nested sub-routes for modularity.
pub fn build_routes(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .nest("/auth", user_routes(state.clone()))
        .nest("/applications", application_routes(state.clone()))
        .nest("/api/clients", client::client_routes(state.clone()))
        .nest(
            "/api/messages",
            instant_message::message_routes(state.clone()),
        )
        .nest("/analytics", analytics_routes(state.clone()))
        //.nest("/apis", api_key_routes(state.clone()))
        .layer(middleware::from_fn(reject_suspicious_query))
        .with_state(state)
}
