use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;
use vaultless_core::models::notification::{
    Notification, NotificationQuery, NotificationSummary, PaginatedNotifications,
    UnreadCountResponse,
};

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

// =============================================================================
// Response Types
// =============================================================================

#[derive(Debug, Serialize, ToSchema)]
pub struct MarkAllReadResponse {
    pub success: bool,
    pub count: i64,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteAllReadResponse {
    pub success: bool,
    pub count: i64,
    pub message: String,
}

// =============================================================================
// Handlers
// =============================================================================

/// List notifications for the current user
#[utoipa::path(
    get,
    path = "/dev/notifications",
    params(
        ("is_read" = Option<bool>, Query, description = "Filter by read status"),
        ("notification_type" = Option<String>, Query, description = "Filter by notification type"),
        ("severity" = Option<String>, Query, description = "Filter by severity"),
        ("page" = Option<i64>, Query, description = "Page number (default: 1)"),
        ("page_size" = Option<i64>, Query, description = "Items per page (default: 20, max: 100)")
    ),
    responses(
        (status = 200, description = "Notifications retrieved successfully", body = PaginatedNotifications,
            example = json!({
                "data": [
                    {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "user_id": "660e8400-e29b-41d4-a716-446655440000",
                        "title": "Quota Warning: My App",
                        "message": "Your application 'My App' has used 85% of its monthly quota.",
                        "notification_type": "quota_warning",
                        "severity": "warning",
                        "action_url": "/dashboard/applications",
                        "metadata": {"app_name": "My App", "usage_percent": 85.0},
                        "is_read": false,
                        "read_at": null,
                        "created_at": "2025-01-15T10:30:00Z",
                        "updated_at": "2025-01-15T10:30:00Z",
                        "expires_at": "2025-01-22T10:30:00Z"
                    }
                ],
                "total_count": 1,
                "page": 1,
                "page_size": 20,
                "total_pages": 1,
                "unread_count": 1
            })
        ),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn list_notifications(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Query(query): Query<NotificationQuery>,
) -> Result<Json<PaginatedNotifications>, ApiError> {
    let notifications = Notification::list_for_user(state.db.as_ref(), session.user_id, query)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(notifications))
}

/// Get a specific notification by ID
#[utoipa::path(
    get,
    path = "/dev/notifications/{notification_id}",
    params(
        ("notification_id" = Uuid, Path, description = "Notification ID")
    ),
    responses(
        (status = 200, description = "Notification retrieved successfully", body = Notification),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Notification not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn get_notification(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(notification_id): Path<Uuid>,
) -> Result<Json<Notification>, ApiError> {
    let notification =
        Notification::find_by_id_for_user(state.db.as_ref(), notification_id, session.user_id)
            .await
            .map_err(ApiError::from)?;

    Ok(Json(notification))
}

/// Get unread notification count
#[utoipa::path(
    get,
    path = "/dev/notifications/unread-count",
    responses(
        (status = 200, description = "Unread count retrieved successfully", body = UnreadCountResponse,
            example = json!({
                "unread_count": 5
            })
        ),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn get_unread_count(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
) -> Result<Json<UnreadCountResponse>, ApiError> {
    let unread_count = Notification::get_unread_count(state.db.as_ref(), session.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(UnreadCountResponse { unread_count }))
}

/// Get notification summary grouped by type and severity
#[utoipa::path(
    get,
    path = "/dev/notifications/summary",
    responses(
        (status = 200, description = "Notification summary retrieved successfully", body = Vec<NotificationSummary>,
            example = json!([
                {
                    "notification_type": "quota_warning",
                    "severity": "warning",
                    "total_count": 3,
                    "unread_count": 2,
                    "latest_notification": "2025-01-15T10:30:00Z"
                },
                {
                    "notification_type": "system_update",
                    "severity": "info",
                    "total_count": 5,
                    "unread_count": 0,
                    "latest_notification": "2025-01-14T08:00:00Z"
                }
            ])
        ),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn get_notification_summary(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
) -> Result<Json<Vec<NotificationSummary>>, ApiError> {
    let summary = Notification::get_summary(state.db.as_ref(), session.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(summary))
}

/// Mark a notification as read
#[utoipa::path(
    post,
    path = "/dev/notifications/{notification_id}/read",
    params(
        ("notification_id" = Uuid, Path, description = "Notification ID")
    ),
    responses(
        (status = 200, description = "Notification marked as read", body = Notification),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Notification not found or already read"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn mark_notification_read(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(notification_id): Path<Uuid>,
) -> Result<Json<Notification>, ApiError> {
    let notification =
        Notification::mark_as_read(state.db.as_ref(), notification_id, session.user_id)
            .await
            .map_err(ApiError::from)?;

    Ok(Json(notification))
}

/// Mark all notifications as read
#[utoipa::path(
    post,
    path = "/dev/notifications/read-all",
    responses(
        (status = 200, description = "All notifications marked as read", body = MarkAllReadResponse,
            example = json!({
                "success": true,
                "count": 5,
                "message": "5 notifications marked as read"
            })
        ),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
) -> Result<Json<MarkAllReadResponse>, ApiError> {
    let count = Notification::mark_all_as_read(state.db.as_ref(), session.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(MarkAllReadResponse {
        success: true,
        count,
        message: format!("{} notifications marked as read", count),
    }))
}

/// Delete a notification
#[utoipa::path(
    delete,
    path = "/dev/notifications/{notification_id}",
    params(
        ("notification_id" = Uuid, Path, description = "Notification ID")
    ),
    responses(
        (status = 200, description = "Notification deleted", body = DeleteResponse,
            example = json!({
                "success": true,
                "message": "Notification deleted successfully"
            })
        ),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Notification not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn delete_notification(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(notification_id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, ApiError> {
    Notification::delete(state.db.as_ref(), notification_id, session.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(DeleteResponse {
        success: true,
        message: "Notification deleted successfully".to_string(),
    }))
}

/// Delete all read notifications
#[utoipa::path(
    delete,
    path = "/dev/notifications/read",
    responses(
        (status = 200, description = "All read notifications deleted", body = DeleteAllReadResponse,
            example = json!({
                "success": true,
                "count": 10,
                "message": "10 read notifications deleted"
            })
        ),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "notifications"
)]
pub async fn delete_all_read_notifications(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
) -> Result<Json<DeleteAllReadResponse>, ApiError> {
    let count = Notification::delete_all_read(state.db.as_ref(), session.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(DeleteAllReadResponse {
        success: true,
        count,
        message: format!("{} read notifications deleted", count),
    }))
}
