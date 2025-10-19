// vaultless-api/src/handlers/notifications.rs
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::middleware::error::ApiError;
use crate::state::AppState;
use vaultless_core::{
    Notification, NotificationFilters, NotificationSeverity, NotificationStats, NotificationType,
};

// ============================================================================
// REQUEST/RESPONSE DTOs
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ListNotificationsQuery {
    pub notification_type: Option<NotificationType>,
    pub severity: Option<NotificationSeverity>,
    pub is_read: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct NotificationResponse {
    pub success: bool,
    pub data: Notification,
}

#[derive(Debug, Serialize)]
pub struct NotificationListResponse {
    pub success: bool,
    pub data: Vec<Notification>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Serialize)]
pub struct PaginationInfo {
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct NotificationStatsResponse {
    pub success: bool,
    pub data: NotificationStats,
}

#[derive(Debug, Serialize)]
pub struct BulkActionResponse {
    pub success: bool,
    pub affected_count: u64,
    pub message: String,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// GET /notifications
/// List notifications with optional filters
pub async fn list_notifications(
    State(state): State<AppState>,
    Query(query): Query<ListNotificationsQuery>,
    user_id: Uuid, // Extracted from JWT token via middleware
) -> Result<impl IntoResponse, ApiError> {
    let filters = NotificationFilters {
        notification_type: query.notification_type,
        severity: query.severity,
        is_read: query.is_read,
        limit: query.limit,
        offset: query.offset,
    };

    let notifications = Notification::list(&state.db, user_id, filters.clone())
        .await
        .map_err(|e| {
            ApiError::internal_server_error(format!("Failed to fetch notifications: {}", e))
        })?;

    // Get total count for pagination
    let total = notifications.len() as i64;
    let limit = filters.limit.unwrap_or(20);
    let offset = filters.offset.unwrap_or(0);
    let has_more = total >= limit;

    Ok(Json(NotificationListResponse {
        success: true,
        data: notifications,
        pagination: PaginationInfo {
            total,
            limit,
            offset,
            has_more,
        },
    }))
}

/// GET /notifications/:id
/// Get a specific notification by ID
pub async fn get_notification(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    let notification = Notification::find_by_id(&state.db, notification_id, user_id)
        .await
        .map_err(|e| match e {
            vaultless_core::error::VaultlessError::NotFound(_) => {
                ApiError::not_found("Notification not found")
            }
            _ => ApiError::internal_server_error(format!("Failed to fetch notification: {}", e)),
        })?;

    Ok(Json(NotificationResponse {
        success: true,
        data: notification,
    }))
}

/// PATCH /notifications/:id/read
/// Mark a notification as read
pub async fn mark_notification_read(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    let notification = Notification::mark_as_read(&state.db, notification_id, user_id)
        .await
        .map_err(|e| match e {
            vaultless_core::error::VaultlessError::NotFound(_) => {
                ApiError::not_found("Notification not found")
            }
            _ => ApiError::internal_server_error(format!("Failed to update notification: {}", e)),
        })?;

    Ok(Json(NotificationResponse {
        success: true,
        data: notification,
    }))
}

/// POST /notifications/mark-all-read
/// Mark all notifications as read for the current user
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    let affected_count = Notification::mark_all_as_read(&state.db, user_id)
        .await
        .map_err(|e| {
            ApiError::internal_server_error(format!("Failed to update notifications: {}", e))
        })?;

    Ok(Json(BulkActionResponse {
        success: true,
        affected_count,
        message: format!("Marked {} notification(s) as read", affected_count),
    }))
}

/// DELETE /notifications/:id
/// Delete a specific notification
pub async fn delete_notification(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    Notification::delete(&state.db, notification_id, user_id)
        .await
        .map_err(|e| match e {
            vaultless_core::error::VaultlessError::NotFound(_) => {
                ApiError::not_found("Notification not found")
            }
            _ => ApiError::internal_server_error(format!("Failed to delete notification: {}", e)),
        })?;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "message": "Notification deleted successfully"
        })),
    ))
}

/// DELETE /notifications/read
/// Delete all read notifications for the current user
pub async fn delete_read_notifications(
    State(state): State<AppState>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    let affected_count = Notification::delete_all_read(&state.db, user_id)
        .await
        .map_err(|e| {
            ApiError::internal_server_error(format!("Failed to delete notifications: {}", e))
        })?;

    Ok(Json(BulkActionResponse {
        success: true,
        affected_count,
        message: format!("Deleted {} read notification(s)", affected_count),
    }))
}

/// GET /notifications/unread/count
/// Get count of unread notifications
pub async fn get_unread_count(
    State(state): State<AppState>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    let count = Notification::get_unread_count(&state.db, user_id)
        .await
        .map_err(|e| {
            ApiError::internal_server_error(format!("Failed to fetch unread count: {}", e))
        })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "unread_count": count
    })))
}

/// GET /notifications/stats
/// Get notification statistics for the current user
pub async fn get_notification_stats(
    State(state): State<AppState>,
    user_id: Uuid,
) -> Result<impl IntoResponse, ApiError> {
    let stats = Notification::get_stats(&state.db, user_id)
        .await
        .map_err(|e| ApiError::internal_server_error(format!("Failed to fetch stats: {}", e)))?;

    Ok(Json(NotificationStatsResponse {
        success: true,
        data: stats,
    }))
}

// ============================================================================
// REAL-TIME NOTIFICATION STREAM (WebSocket or SSE)
// ============================================================================

/// GET /notifications/stream
/// Server-Sent Events stream for real-time notifications (Pro+ feature)
pub async fn notification_stream(
    State(state): State<AppState>,
    user_id: Uuid,
    user_tier: vaultless_core::SubscriptionTier,
) -> Result<impl IntoResponse, ApiError> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream::{self, StreamExt}; // ✅ include StreamExt for `.then`
    use std::convert::Infallible;
    use std::time::Duration;

    // ✅ Only Pro or Enterprise users can use the real-time stream
    if !matches!(
        user_tier,
        vaultless_core::SubscriptionTier::Pro | vaultless_core::SubscriptionTier::Enterprise
    ) {
        return Err(ApiError::forbidden(
            "Real-time notifications require Pro tier or higher",
        ));
    }

    // ✅ Clone the database pool (cheap because PgPool uses Arc internally)
    let db = state.db.clone();

    // ✅ Create an async stream that polls for unread notifications
    let stream = stream::repeat_with(move || {
        let db = db.clone(); // Clone per iteration to satisfy async ownership
        let user_id = user_id; // capture user_id by copy (Uuid implements Copy)
        async move {
            // Poll every 5 seconds
            tokio::time::sleep(Duration::from_secs(5)).await;

            // Fetch unread notifications
            let notifications = Notification::list(
                &db,
                user_id,
                NotificationFilters {
                    is_read: Some(false),
                    limit: Some(10),
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_default();

            if !notifications.is_empty() {
                let json = serde_json::to_string(&notifications).unwrap_or_default();
                Ok::<_, Infallible>(Event::default().data(json))
            } else {
                // Heartbeat event (keeps SSE alive)
                Ok::<_, Infallible>(Event::default().comment("heartbeat"))
            }
        }
    })
    .then(|f| f); // `.then()` requires StreamExt

    // ✅ Return SSE response
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
