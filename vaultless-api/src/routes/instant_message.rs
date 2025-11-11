use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{
    AppState,
    handlers::instant_message::*,
    middleware::{application, client::require_authenticated_client},
};

pub fn message_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Message operations (require authentication)
        .route("/send", post(send_message))
        .route("/inbox", get(fetch_inbox))
        .route("/{message_id}/read", post(mark_message_read))
        .route("/{message_id}/receipts", get(get_read_receipts))
        // Health check (no auth required)
        .route("/health", get(message_health_check))
        // Apply middleware globally for all routes above
        .layer(middleware::from_fn_with_state(
            state.clone(),
            application::check_quota,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_authenticated_client,
        ))
}
