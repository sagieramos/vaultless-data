use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{
    AppState,
    handlers::clients::instant_message::*,
    handlers::clients::instant_message_ws::websocket_handler,
    middleware::{application, client::client_auth},
};

pub fn message_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Message operations (require authentication)
        .route("/send", post(send_message))
        .route("/inbox", get(fetch_inbox))
        .route("/{message_id}/read", post(mark_message_read))
        .route("/{message_id}/receipts", get(get_read_receipts))
        // WebSocket endpoint for real-time messaging
        .route("/ws", get(websocket_handler))
        // Health check (no auth required)
        .route("/health", get(message_health_check))
        // Apply middleware globally for all routes above
        .layer(middleware::from_fn_with_state(
            state.clone(),
            application::app_auth,
        ))
        .layer(middleware::from_fn_with_state(state.clone(), client_auth))
}
