pub mod application_route;
pub mod client;
pub mod health;
pub mod instant_message;
pub mod limits;
pub mod notification;
//pub mod proof;
pub mod user;

use axum::{Router, extract::DefaultBodyLimit, middleware, routing::get};

use application_route::application_routes;
use client::client_routes;
use instant_message::message_routes;
use notification::notification_routes;
use user::user_routes;

use crate::{middleware::global::reject_suspicious_query, state::AppState};

pub fn build_routes(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check))
        .route("/live", get(health::liveness_check))
        .route("/check_cache", get(health::check_cache_handler))
        .nest(
            "/dev",
            Router::new()
                .nest("/auth", user_routes(state.clone()))
                .nest("/applications", application_routes(state.clone()))
                .nest("/notifications", notification_routes(state.clone())),
        )
        .nest(
            "/api/v1",
            Router::new()
                .nest("/clients", client_routes(state.clone()))
                .nest("/messages", message_routes(state.clone())),
        )
        .layer(middleware::from_fn(reject_suspicious_query))
        .layer(DefaultBodyLimit::max(limits::MAX_REQUEST_SIZE))
        .with_state(state)
}
