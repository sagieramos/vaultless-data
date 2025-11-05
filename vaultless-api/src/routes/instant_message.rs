use axum::{
    routing::{get, post},
    Router,
};

use crate::{handlers::instant_message::*, AppState};

pub fn message_routes() -> Router<AppState> {
    Router::new()
        // Message operations (require authentication)
        .route("/send", post(send_message))
        .route("/inbox", get(fetch_inbox))
        .route("/{message_id}/read", post(mark_message_read))
        .route("/{message_id}/receipts", get(get_read_receipts))
        // Health check (no auth required)
        .route("/health", get(message_health_check))
}