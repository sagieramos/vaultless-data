// vaultless-api/src/routes/notifications.rs
use crate::{
    handlers::notifications::*, middleware::token_auth::require_user_auth, state::AppState,
};

use axum::{
    Router,
    routing::{delete, get, patch, post},
};

/// Build notification routes
/// All routes require user authentication (JWT token)
pub fn notification_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // List and filter notifications
        .route("/", get(list_notifications))
        // Get specific notification
        .route("/:id", get(get_notification))
        // Mark as read
        .route("/:id/read", patch(mark_notification_read))
        // Bulk mark all as read
        .route("/mark-all-read", post(mark_all_notifications_read))
        // Delete notification
        .route("/:id", delete(delete_notification))
        // Bulk delete read notifications
        .route("/read", delete(delete_read_notifications))
        // Unread count (for badge)
        .route("/unread/count", get(get_unread_count))
        // Statistics
        .route("/stats", get(get_notification_stats))
        // Real-time stream (SSE) - Pro+ only
        .route("/stream", get(notification_stream))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_user_auth,
        ))
}
