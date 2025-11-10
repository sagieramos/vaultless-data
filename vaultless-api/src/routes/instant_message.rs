use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{
    AppState, handlers::instant_message::*, 
    middleware::{client::authenticate_client_middleware, application},
};

pub fn message_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Message operations (require authentication)
        .route("/send", post(send_message))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_client_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            application::check_quota,
        ))
        .route("/inbox", get(fetch_inbox))
        .route("/{message_id}/read", post(mark_message_read))
        .route("/{message_id}/receipts", get(get_read_receipts))
        // Health check (no auth required)
        .route("/health", get(message_health_check))
}