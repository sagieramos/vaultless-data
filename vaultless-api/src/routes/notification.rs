use axum::{
    Router, middleware,
    routing::{delete, get, post},
};

use crate::{
    handlers::developer::notification::*,
    middleware::user::user_auth,
    state::AppState,
};

pub fn notification_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Get unread count (specific route first to avoid path conflicts)
        .route("/unread-count", get(get_unread_count))
        // Get notification summary
        .route("/summary", get(get_notification_summary))
        // Mark all as read
        .route("/read-all", post(mark_all_notifications_read))
        // Delete all read notifications
        .route("/read", delete(delete_all_read_notifications))
        // List all notifications
        .route("/", get(list_notifications))
        // Get specific notification
        .route("/{notification_id}", get(get_notification))
        // Mark specific notification as read
        .route("/{notification_id}/read", post(mark_notification_read))
        // Delete specific notification
        .route("/{notification_id}", delete(delete_notification))
        // Apply authentication middleware
        .layer(middleware::from_fn_with_state(state.clone(), user_auth))
}
