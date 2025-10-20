// vaultless-api/src/handlers/notifications.rs
use crate::{middleware::error::ApiError, services::token::SessionData, state::AppState};
use axum::extract::Extension;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
#[axum::debug_handler]
pub async fn list_notifications(
    State(state): State<AppState>,
    Query(query): Query<ListNotificationsQuery>,
    Extension(user_id): Extension<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let filters = NotificationFilters {
        notification_type: query.notification_type,
        severity: query.severity,
        is_read: query.is_read,
        limit: query.limit,
        offset: query.offset,
        since: None,
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
#[axum::debug_handler]
pub async fn get_notification(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

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
#[axum::debug_handler]
pub async fn mark_notification_read(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

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
#[axum::debug_handler]
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

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
#[axum::debug_handler]
pub async fn delete_notification(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

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
#[axum::debug_handler]
pub async fn delete_read_notifications(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

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
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

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
#[axum::debug_handler]
pub async fn get_notification_stats(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>,
) -> Result<impl IntoResponse, ApiError> {
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
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

/// GET /notification/stream
/// Server-Sent Events for real-time notifications (Pro+ feature)
pub async fn notification_stream(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>, // Extracted by Vaultless middleware
) -> Result<impl IntoResponse, ApiError> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::stream;
    use std::convert::Infallible;
    use std::time::Duration;
    use vaultless_core::SubscriptionTier;

    // Only Pro or Enterprise users allowed
    if let Some(tier_str) = &session.scope {
        let tier: SubscriptionTier = tier_str
            .parse()
            .map_err(|_| ApiError::forbidden("Invalid subscription tier"))?;

        if !matches!(tier, SubscriptionTier::Pro | SubscriptionTier::Enterprise)
            && !session.is_admin
        {
            return Err(ApiError::forbidden(
                "Real-time notifications require Pro tier or higher",
            ));
        }
    } else if !session.is_admin {
        return Err(ApiError::forbidden(
            "Real-time notifications require Pro tier or higher",
        ));
    }

    let db = state.db.clone();
    let user_id = Uuid::parse_str(&session.user_id)
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    // Keep track of last seen notification timestamp
    let mut last_checked = Utc::now();

    // Configurable poll interval
    let poll_interval = Duration::from_secs(5);

    let stream = stream::unfold((), move |_| {
        let db = db.clone();
        let user_id = user_id;
        async move {
            tokio::time::sleep(poll_interval).await;

            let filters = NotificationFilters {
                is_read: Some(false),
                limit: Some(10),
                since: Some(last_checked),
                ..Default::default()
            };

            let notifications = match Notification::list(&db, user_id, filters).await {
                Ok(list) => list,
                Err(e) => {
                    tracing::error!("Failed to fetch notifications: {}", e);
                    vec![]
                }
            };

            // Update last_checked if we have new notifications
            if let Some(last) = notifications.last() {
                last_checked = last.created_at; // assumes Notification has a `created_at: DateTime<Utc>` field
            }

            let event = if !notifications.is_empty() {
                let json = serde_json::to_string(&notifications).unwrap_or_default();
                Event::default().data(json)
            } else {
                Event::default().comment("heartbeat")
            };

            Some((Ok::<_, Infallible>(event), ()))
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
