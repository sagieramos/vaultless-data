use chrono::{DateTime, Datelike, Timelike, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Row};
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::cache_key;
use crate::error::{Result, VaultlessError};

// =============================================================================
// Enums (matching PostgreSQL types)
// =============================================================================

/// Type of notification for categorization and filtering
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, ToSchema)]
#[sqlx(type_name = "notification_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    QuotaWarning,
    QuotaExceeded,
    BillingAlert,
    SecurityAlert,
    SystemUpdate,
    MarketingOffer,
    ApiKeyExpiring,
    UsageReport,
}

impl std::fmt::Display for NotificationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QuotaWarning => write!(f, "quota_warning"),
            Self::QuotaExceeded => write!(f, "quota_exceeded"),
            Self::BillingAlert => write!(f, "billing_alert"),
            Self::SecurityAlert => write!(f, "security_alert"),
            Self::SystemUpdate => write!(f, "system_update"),
            Self::MarketingOffer => write!(f, "marketing_offer"),
            Self::ApiKeyExpiring => write!(f, "api_key_expiring"),
            Self::UsageReport => write!(f, "usage_report"),
        }
    }
}

/// Severity level of the notification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, ToSchema)]
#[sqlx(type_name = "notification_severity", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for NotificationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

// =============================================================================
// Main Notification Model
// =============================================================================

/// Notification entity matching the database schema
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,

    // Content
    pub title: String,
    pub message: String,

    // Classification
    pub notification_type: NotificationType,
    pub severity: NotificationSeverity,

    // Action
    #[schema(example = "/dashboard/upgrade")]
    pub action_url: Option<String>,

    // Metadata
    pub metadata: Option<serde_json::Value>,

    // Status
    pub is_read: bool,
    pub read_at: Option<DateTime<Utc>>,

    // Lifecycle
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

// =============================================================================
// DTOs (Data Transfer Objects)
// =============================================================================

/// Request to create a new notification
#[derive(Debug, Clone, Deserialize, Validate, ToSchema)]
pub struct CreateNotification {
    pub user_id: Uuid,

    #[validate(length(min = 1, max = 255))]
    pub title: String,

    #[validate(length(min = 1, max = 5000))]
    pub message: String,

    pub notification_type: NotificationType,

    #[serde(default = "default_severity")]
    pub severity: NotificationSeverity,

    #[validate(length(max = 500))]
    pub action_url: Option<String>,

    pub metadata: Option<serde_json::Value>,

    pub expires_at: Option<DateTime<Utc>>,
}

fn default_severity() -> NotificationSeverity {
    NotificationSeverity::Info
}

/// Request to update notification (mainly for marking as read)
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateNotification {
    pub is_read: Option<bool>,
}

/// Query parameters for listing notifications
#[derive(Debug, Clone, Deserialize, Validate, ToSchema)]
pub struct NotificationQuery {
    /// Filter by read status
    pub is_read: Option<bool>,

    /// Filter by notification type
    pub notification_type: Option<NotificationType>,

    /// Filter by severity
    pub severity: Option<NotificationSeverity>,

    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    #[validate(range(min = 1))]
    pub page: i64,

    /// Items per page
    #[serde(default = "default_page_size")]
    #[validate(range(min = 1, max = 100))]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    20
}

/// Paginated notification response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PaginatedNotifications {
    pub data: Vec<Notification>,
    pub total_count: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
    pub unread_count: i64,
}

/// Summary of notifications by type and severity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct NotificationSummary {
    pub notification_type: NotificationType,
    pub severity: NotificationSeverity,
    pub total_count: i64,
    pub unread_count: i64,
    pub latest_notification: Option<DateTime<Utc>>,
}

/// Unread count response
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UnreadCountResponse {
    pub unread_count: i64,
}

// =============================================================================
// Implementation
// =============================================================================

impl Notification {
    /// Create a new notification
    pub async fn create(pool: &PgPool, input: CreateNotification) -> Result<Self> {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let notification = sqlx::query_as::<_, Notification>(
            r#"
            INSERT INTO notifications (
                user_id, title, message, notification_type, severity,
                action_url, metadata, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(input.user_id)
        .bind(&input.title)
        .bind(&input.message)
        .bind(input.notification_type)
        .bind(input.severity)
        .bind(&input.action_url)
        .bind(&input.metadata)
        .bind(input.expires_at)
        .fetch_one(pool)
        .await?;

        tracing::info!(
            notification_id = %notification.id,
            user_id = %notification.user_id,
            notification_type = %notification.notification_type,
            severity = %notification.severity,
            "Notification created"
        );

        Ok(notification)
    }

    /// Find notification by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        sqlx::query_as::<_, Notification>(
            r#"
            SELECT * FROM notifications
            WHERE id = $1 AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Notification not found".to_string()))
    }

    /// Find notification by ID for a specific user (ownership check)
    pub async fn find_by_id_for_user(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<Self> {
        sqlx::query_as::<_, Notification>(
            r#"
            SELECT * FROM notifications
            WHERE id = $1 AND user_id = $2 AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Notification not found".to_string()))
    }

    /// List notifications for a user with filtering and pagination
    pub async fn list_for_user(
        pool: &PgPool,
        user_id: Uuid,
        query: NotificationQuery,
    ) -> Result<PaginatedNotifications> {
        let offset = (query.page - 1) * query.page_size;

        // Build dynamic WHERE clause
        let mut conditions = vec![
            "user_id = $1".to_string(),
            "(expires_at IS NULL OR expires_at > NOW())".to_string(),
        ];
        let mut param_idx = 2;

        if query.is_read.is_some() {
            conditions.push(format!("is_read = ${}", param_idx));
            param_idx += 1;
        }

        if query.notification_type.is_some() {
            conditions.push(format!("notification_type = ${}", param_idx));
            param_idx += 1;
        }

        if query.severity.is_some() {
            conditions.push(format!("severity = ${}", param_idx));
            param_idx += 1;
        }

        let where_clause = conditions.join(" AND ");

        // Count query
        let count_sql = format!(
            "SELECT COUNT(*) as count FROM notifications WHERE {}",
            where_clause
        );

        // Data query
        let data_sql = format!(
            r#"
            SELECT * FROM notifications
            WHERE {}
            ORDER BY created_at DESC
            LIMIT ${} OFFSET ${}
            "#,
            where_clause, param_idx, param_idx + 1
        );

        // Build and execute count query
        let mut count_query = sqlx::query(&count_sql).bind(user_id);
        if let Some(is_read) = query.is_read {
            count_query = count_query.bind(is_read);
        }
        if let Some(notification_type) = query.notification_type {
            count_query = count_query.bind(notification_type);
        }
        if let Some(severity) = query.severity {
            count_query = count_query.bind(severity);
        }

        let total_count: i64 = count_query
            .fetch_one(pool)
            .await?
            .get("count");

        // Build and execute data query
        let mut data_query = sqlx::query_as::<_, Notification>(&data_sql).bind(user_id);
        if let Some(is_read) = query.is_read {
            data_query = data_query.bind(is_read);
        }
        if let Some(notification_type) = query.notification_type {
            data_query = data_query.bind(notification_type);
        }
        if let Some(severity) = query.severity {
            data_query = data_query.bind(severity);
        }
        data_query = data_query.bind(query.page_size).bind(offset);

        let data = data_query.fetch_all(pool).await?;

        // Get unread count
        let unread_count = Self::get_unread_count(pool, user_id).await?;

        let total_pages = (total_count as f64 / query.page_size as f64).ceil() as i64;

        Ok(PaginatedNotifications {
            data,
            total_count,
            page: query.page,
            page_size: query.page_size,
            total_pages,
            unread_count,
        })
    }

    /// Get unread notification count for a user
    pub async fn get_unread_count(pool: &PgPool, user_id: Uuid) -> Result<i64> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count FROM notifications
            WHERE user_id = $1
              AND is_read = FALSE
              AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(row.get("count"))
    }

    /// Mark notification as read
    pub async fn mark_as_read(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<Self> {
        let notification = sqlx::query_as::<_, Notification>(
            r#"
            UPDATE notifications
            SET is_read = TRUE
            WHERE id = $1 AND user_id = $2 AND is_read = FALSE
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| {
            VaultlessError::NotFound("Notification not found or already read".to_string())
        })?;

        tracing::debug!(
            notification_id = %id,
            user_id = %user_id,
            "Notification marked as read"
        );

        Ok(notification)
    }

    /// Mark all notifications as read for a user
    pub async fn mark_all_as_read(pool: &PgPool, user_id: Uuid) -> Result<i64> {
        let result = sqlx::query(
            r#"
            UPDATE notifications
            SET is_read = TRUE
            WHERE user_id = $1 AND is_read = FALSE
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        let count = result.rows_affected() as i64;

        tracing::info!(
            user_id = %user_id,
            count = count,
            "All notifications marked as read"
        );

        Ok(count)
    }

    /// Delete a notification
    pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<()> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(VaultlessError::NotFound(
                "Notification not found".to_string(),
            ));
        }

        tracing::debug!(
            notification_id = %id,
            user_id = %user_id,
            "Notification deleted"
        );

        Ok(())
    }

    /// Delete all read notifications for a user
    pub async fn delete_all_read(pool: &PgPool, user_id: Uuid) -> Result<i64> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications
            WHERE user_id = $1 AND is_read = TRUE
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        let count = result.rows_affected() as i64;

        tracing::info!(
            user_id = %user_id,
            count = count,
            "All read notifications deleted"
        );

        Ok(count)
    }

    /// Get notification summary for a user
    pub async fn get_summary(pool: &PgPool, user_id: Uuid) -> Result<Vec<NotificationSummary>> {
        let summaries = sqlx::query_as::<_, NotificationSummary>(
            r#"
            SELECT
                notification_type,
                severity,
                COUNT(*)::bigint as total_count,
                COUNT(*) FILTER (WHERE is_read = FALSE)::bigint as unread_count,
                MAX(created_at) as latest_notification
            FROM notifications
            WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW())
            GROUP BY notification_type, severity
            ORDER BY latest_notification DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await?;

        Ok(summaries)
    }

    /// Cleanup expired notifications (admin function)
    pub async fn cleanup_expired(pool: &PgPool) -> Result<i64> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications
            WHERE expires_at IS NOT NULL AND expires_at < NOW()
            "#,
        )
        .execute(pool)
        .await?;

        let count = result.rows_affected() as i64;

        tracing::info!(count = count, "Expired notifications cleaned up");

        Ok(count)
    }

    /// Cleanup old read notifications (admin function)
    pub async fn cleanup_old_read(pool: &PgPool, retention_days: i32) -> Result<i64> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications
            WHERE is_read = TRUE
              AND read_at < NOW() - ($1 || ' days')::INTERVAL
            "#,
        )
        .bind(retention_days)
        .execute(pool)
        .await?;

        let count = result.rows_affected() as i64;

        tracing::info!(
            retention_days = retention_days,
            count = count,
            "Old read notifications cleaned up"
        );

        Ok(count)
    }
}

// =============================================================================
// Notification Builder (for convenience)
// =============================================================================

/// Builder for creating notifications with a fluent API
pub struct NotificationBuilder {
    user_id: Uuid,
    title: String,
    message: String,
    notification_type: NotificationType,
    severity: NotificationSeverity,
    action_url: Option<String>,
    metadata: Option<serde_json::Value>,
    expires_at: Option<DateTime<Utc>>,
}

impl NotificationBuilder {
    pub fn new(
        user_id: Uuid,
        title: impl Into<String>,
        message: impl Into<String>,
        notification_type: NotificationType,
    ) -> Self {
        Self {
            user_id,
            title: title.into(),
            message: message.into(),
            notification_type,
            severity: NotificationSeverity::Info,
            action_url: None,
            metadata: None,
            expires_at: None,
        }
    }

    pub fn severity(mut self, severity: NotificationSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn action_url(mut self, url: impl Into<String>) -> Self {
        self.action_url = Some(url.into());
        self
    }

    pub fn metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn expires_in_days(mut self, days: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::days(days));
        self
    }

    pub fn build(self) -> CreateNotification {
        CreateNotification {
            user_id: self.user_id,
            title: self.title,
            message: self.message,
            notification_type: self.notification_type,
            severity: self.severity,
            action_url: self.action_url,
            metadata: self.metadata,
            expires_at: self.expires_at,
        }
    }

    pub async fn send(self, pool: &PgPool) -> Result<Notification> {
        Notification::create(pool, self.build()).await
    }
}

// =============================================================================
// Helper Functions for Common Notifications
// =============================================================================

impl Notification {
    /// Create a quota warning notification
    pub async fn send_quota_warning(
        pool: &PgPool,
        user_id: Uuid,
        app_name: &str,
        usage_percent: f64,
    ) -> Result<Self> {
        let severity = if usage_percent >= 95.0 {
            NotificationSeverity::Critical
        } else if usage_percent >= 80.0 {
            NotificationSeverity::Warning
        } else {
            NotificationSeverity::Info
        };

        NotificationBuilder::new(
            user_id,
            format!("Quota Warning: {}", app_name),
            format!(
                "Your application '{}' has used {:.1}% of its monthly quota.",
                app_name, usage_percent
            ),
            NotificationType::QuotaWarning,
        )
        .severity(severity)
        .action_url("/dashboard/applications")
        .metadata(serde_json::json!({
            "app_name": app_name,
            "usage_percent": usage_percent
        }))
        .expires_in_days(7)
        .send(pool)
        .await
    }

    /// Create an API key expiring notification
    pub async fn send_api_key_expiring(
        pool: &PgPool,
        user_id: Uuid,
        key_prefix: &str,
        days_until_expiry: i64,
    ) -> Result<Self> {
        let severity = if days_until_expiry <= 3 {
            NotificationSeverity::Critical
        } else if days_until_expiry <= 7 {
            NotificationSeverity::Warning
        } else {
            NotificationSeverity::Info
        };

        NotificationBuilder::new(
            user_id,
            "API Key Expiring Soon",
            format!(
                "Your API key ({}) will expire in {} days. Please rotate it to avoid service interruption.",
                key_prefix, days_until_expiry
            ),
            NotificationType::ApiKeyExpiring,
        )
        .severity(severity)
        .action_url("/dashboard/keys")
        .metadata(serde_json::json!({
            "key_prefix": key_prefix,
            "days_until_expiry": days_until_expiry
        }))
        .expires_in_days(days_until_expiry)
        .send(pool)
        .await
    }

    /// Create a security alert notification
    pub async fn send_security_alert(
        pool: &PgPool,
        user_id: Uuid,
        title: &str,
        message: &str,
    ) -> Result<Self> {
        NotificationBuilder::new(
            user_id,
            title,
            message,
            NotificationType::SecurityAlert,
        )
        .severity(NotificationSeverity::Critical)
        .action_url("/dashboard/security")
        .expires_in_days(30)
        .send(pool)
        .await
    }

    /// Create a welcome notification for new users
    pub async fn send_welcome(pool: &PgPool, user_id: Uuid) -> Result<Self> {
        NotificationBuilder::new(
            user_id,
            "Welcome to Vaultless!",
            "Get started by creating your first application and API key.",
            NotificationType::SystemUpdate,
        )
        .severity(NotificationSeverity::Info)
        .action_url("/dashboard/applications/new")
        .metadata(serde_json::json!({ "welcome": true }))
        .expires_in_days(30)
        .send(pool)
        .await
    }
}

// =============================================================================
// Notification Event Tracking (Redis-based deduplication)
// =============================================================================

/// Track rate limit and quota events in Redis for daily aggregation.
/// This prevents sending duplicate notifications within the same day.
pub struct NotificationEventTracker;

impl NotificationEventTracker {
    /// Redis key for tracking daily rate limit hits per API key
    fn rate_limit_key(api_key_id: Uuid, date: &str) -> String {
        cache_key!("notify", "rate_limit", api_key_id, date)
    }

    /// Redis key for tracking if quota exceeded notification was sent today
    fn quota_exceeded_key(api_key_id: Uuid, date: &str) -> String {
        cache_key!("notify", "quota_exceeded", api_key_id, date)
    }

    /// Redis key for tracking quota warning thresholds already notified
    fn quota_warning_key(api_key_id: Uuid, date: &str, threshold: u8) -> String {
        cache_key!("notify", "quota_warning", api_key_id, date, threshold)
    }

    /// Get today's date string in YYYYMMDD format
    pub fn today_date_str() -> String {
        let now = Utc::now();
        format!("{:04}{:02}{:02}", now.year(), now.month(), now.day())
    }

    /// TTL for notification tracking keys (25 hours to cover timezone differences)
    const KEY_TTL_SECS: i64 = 25 * 60 * 60;

    /// Increment rate limit hit counter for today.
    /// Returns the new count of rate limit hits for today.
    pub async fn increment_rate_limit_hits(
        redis_pool: &Arc<RedisPool>,
        api_key_id: Uuid,
    ) -> Result<i64> {
        let date = Self::today_date_str();
        let key = Self::rate_limit_key(api_key_id, &date);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // Atomically increment and set TTL if key is new
        let count: i64 = redis::pipe()
            .atomic()
            .incr(&key, 1)
            .expire(&key, Self::KEY_TTL_SECS)
            .ignore()
            .query_async(&mut *conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        Ok(count)
    }

    /// Get current rate limit hit count for today
    pub async fn get_rate_limit_hits(
        redis_pool: &Arc<RedisPool>,
        api_key_id: Uuid,
    ) -> Result<i64> {
        let date = Self::today_date_str();
        let key = Self::rate_limit_key(api_key_id, &date);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let count: Option<i64> = conn
            .get(&key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        Ok(count.unwrap_or(0))
    }

    /// Check if quota exceeded notification was already sent today.
    /// If not, mark it as sent and return false. If already sent, return true.
    pub async fn check_and_mark_quota_exceeded(
        redis_pool: &Arc<RedisPool>,
        api_key_id: Uuid,
    ) -> Result<bool> {
        let date = Self::today_date_str();
        let key = Self::quota_exceeded_key(api_key_id, &date);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // SETNX returns 1 if key was set (notification not sent yet), 0 if already exists
        let was_set: bool = redis::cmd("SET")
            .arg(&key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(Self::KEY_TTL_SECS)
            .query_async(&mut *conn)
            .await
            .map(|result: Option<String>| result.is_some())
            .unwrap_or(false);

        // If was_set is true, notification was NOT sent yet (we just set it)
        // If was_set is false, notification was already sent
        Ok(!was_set)
    }

    /// Check if quota warning for a specific threshold was already sent today.
    /// Thresholds: 80, 90, 95, 100 (percentage)
    pub async fn check_and_mark_quota_warning(
        redis_pool: &Arc<RedisPool>,
        api_key_id: Uuid,
        threshold: u8,
    ) -> Result<bool> {
        let date = Self::today_date_str();
        let key = Self::quota_warning_key(api_key_id, &date, threshold);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let was_set: bool = redis::cmd("SET")
            .arg(&key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(Self::KEY_TTL_SECS)
            .query_async(&mut *conn)
            .await
            .map(|result: Option<String>| result.is_some())
            .unwrap_or(false);

        Ok(!was_set)
    }

    /// Get all API keys that had rate limit hits today (for daily summary job)
    pub async fn get_api_keys_with_rate_limit_hits(
        redis_pool: &Arc<RedisPool>,
    ) -> Result<Vec<(Uuid, i64)>> {
        let date = Self::today_date_str();
        let pattern = cache_key!("notify", "rate_limit", "*", date);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        // Scan for all rate limit keys for today
        let mut cursor: u64 = 0;
        let mut results: Vec<(Uuid, i64)> = Vec::new();

        loop {
            let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut *conn)
                .await
                .map_err(|e| VaultlessError::Internal(e.to_string()))?;

            for key in keys {
                // Extract UUID from key: vaultless:notify:rate_limit:{uuid}:{date}
                if let Some(uuid_str) = key.split(':').nth(3) {
                    if let Ok(uuid) = Uuid::parse_str(uuid_str) {
                        let count: Option<i64> = conn
                            .get(&key)
                            .await
                            .map_err(|e| VaultlessError::Internal(e.to_string()))?;
                        if let Some(count) = count {
                            if count > 0 {
                                results.push((uuid, count));
                            }
                        }
                    }
                }
            }

            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }

        Ok(results)
    }

    /// Reset rate limit hit counter (called after sending daily summary)
    pub async fn reset_rate_limit_hits(
        redis_pool: &Arc<RedisPool>,
        api_key_id: Uuid,
    ) -> Result<()> {
        let date = Self::today_date_str();
        let key = Self::rate_limit_key(api_key_id, &date);

        let mut conn = redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        let _: () = conn
            .del(&key)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        Ok(())
    }
}

// =============================================================================
// Rate Limit and Quota Notification Helpers
// =============================================================================

impl Notification {
    /// Send rate limit hit notification (daily summary).
    /// This should be called by a background job, not on every rate limit hit.
    pub async fn send_rate_limit_summary(
        pool: &PgPool,
        user_id: Uuid,
        app_name: &str,
        app_id: Uuid,
        hit_count: i64,
    ) -> Result<Self> {
        let severity = if hit_count >= 100 {
            NotificationSeverity::Critical
        } else if hit_count >= 50 {
            NotificationSeverity::Warning
        } else {
            NotificationSeverity::Info
        };

        NotificationBuilder::new(
            user_id,
            format!("Rate Limit Alert: {}", app_name),
            format!(
                "Your application '{}' hit the rate limit {} times today. Consider upgrading your plan or optimizing your request patterns.",
                app_name, hit_count
            ),
            NotificationType::SecurityAlert,
        )
        .severity(severity)
        .action_url("/dashboard/applications")
        .metadata(serde_json::json!({
            "app_name": app_name,
            "app_id": app_id,
            "rate_limit_hits": hit_count,
            "event_type": "rate_limit_summary"
        }))
        .expires_in_days(7)
        .send(pool)
        .await
    }

    /// Send quota exceeded notification (once per day).
    pub async fn send_quota_exceeded(
        pool: &PgPool,
        user_id: Uuid,
        app_name: &str,
        app_id: Uuid,
    ) -> Result<Self> {
        NotificationBuilder::new(
            user_id,
            format!("Quota Exceeded: {}", app_name),
            format!(
                "Your application '{}' has exceeded its monthly message quota. API requests are being rejected. Please upgrade your plan to restore service.",
                app_name
            ),
            NotificationType::QuotaExceeded,
        )
        .severity(NotificationSeverity::Critical)
        .action_url("/dashboard/upgrade")
        .metadata(serde_json::json!({
            "app_name": app_name,
            "app_id": app_id,
            "event_type": "quota_exceeded"
        }))
        .expires_in_days(30)
        .send(pool)
        .await
    }
}

// =============================================================================
// Data structures for notification job
// =============================================================================

/// Information needed to send a rate limit notification
#[derive(Debug, Clone)]
pub struct RateLimitNotificationData {
    pub api_key_id: Uuid,
    pub user_id: Uuid,
    pub app_id: Uuid,
    pub app_name: String,
    pub hit_count: i64,
}

/// Query to get application info from API key ID for notification purposes
impl Notification {
    /// Get application details needed for sending notifications
    pub async fn get_app_info_for_notification(
        pool: &PgPool,
        api_key_id: Uuid,
    ) -> Result<Option<(Uuid, Uuid, String)>> {
        // Returns (app_id, user_id, app_name)
        let result = sqlx::query_as::<_, (Uuid, Uuid, String)>(
            r#"
            SELECT a.id, a.developer_id, a.name
            FROM applications a
            WHERE a.secret_key_id = $1
            "#,
        )
        .bind(api_key_id)
        .fetch_optional(pool)
        .await?;

        Ok(result)
    }
}

// =============================================================================
// Background Notification Job
// =============================================================================

use sqlx::PgPool as SqlxPgPool;
use tokio::sync::Notify;
use tokio::time::{Duration, interval};

/// Configuration for the notification job
#[derive(Clone, Debug)]
pub struct NotificationJobConfig {
    /// How often to check for notifications to send (in seconds)
    /// Default: 3600 (1 hour)
    pub check_interval_secs: u64,
    /// Hour of day (0-23) to send daily rate limit summaries
    /// Default: 9 (9 AM UTC)
    pub daily_summary_hour: u32,
}

impl Default for NotificationJobConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: 3600, // Check every hour
            daily_summary_hour: 9,     // 9 AM UTC
        }
    }
}

/// Start the background notification job
/// This job:
/// 1. Sends rate limit summary notifications once per day
/// 2. Sends quota exceeded notifications when flagged
pub fn start_notification_job(
    redis_pool: Arc<RedisPool>,
    pg_pool: Arc<SqlxPgPool>,
    config: NotificationJobConfig,
) -> (tokio::task::JoinHandle<()>, Arc<Notify>) {
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);

    let handle = tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(config.check_interval_secs));
        let mut last_daily_run: Option<chrono::NaiveDate> = None;

        tracing::info!(
            check_interval_secs = config.check_interval_secs,
            daily_summary_hour = config.daily_summary_hour,
            "Notification job started"
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let now = Utc::now();
                    let today = now.date_naive();

                    // Check if we should run the daily summary
                    let should_run_daily = match last_daily_run {
                        None => now.hour() >= config.daily_summary_hour,
                        Some(last_date) => {
                            last_date < today && now.hour() >= config.daily_summary_hour
                        }
                    };

                    if should_run_daily {
                        tracing::info!("Running daily notification summary job");

                        if let Err(e) = process_rate_limit_notifications(
                            &redis_pool,
                            &pg_pool,
                        ).await {
                            tracing::error!(?e, "Failed to process rate limit notifications");
                        }

                        if let Err(e) = process_quota_exceeded_notifications(
                            &redis_pool,
                            &pg_pool,
                        ).await {
                            tracing::error!(?e, "Failed to process quota exceeded notifications");
                        }

                        last_daily_run = Some(today);
                        tracing::info!("Daily notification summary completed");
                    }
                }
                _ = shutdown.notified() => {
                    tracing::info!("Notification job shutdown signal received");
                    break;
                }
            }
        }
    });

    (handle, shutdown_clone)
}

/// Process and send rate limit summary notifications
async fn process_rate_limit_notifications(
    redis_pool: &Arc<RedisPool>,
    pg_pool: &SqlxPgPool,
) -> Result<()> {
    // Get all API keys that had rate limit hits today
    let api_keys_with_hits = NotificationEventTracker::get_api_keys_with_rate_limit_hits(redis_pool)
        .await?;

    tracing::info!(
        count = api_keys_with_hits.len(),
        "Found API keys with rate limit hits"
    );

    for (api_key_id, hit_count) in api_keys_with_hits {
        // Get app info for notification
        if let Ok(Some((app_id, user_id, app_name))) =
            Notification::get_app_info_for_notification(pg_pool, api_key_id).await
        {
            // Send notification
            match Notification::send_rate_limit_summary(
                pg_pool,
                user_id,
                &app_name,
                app_id,
                hit_count,
            )
            .await
            {
                Ok(notification) => {
                    tracing::info!(
                        notification_id = %notification.id,
                        user_id = %user_id,
                        app_name = %app_name,
                        hit_count = hit_count,
                        "Sent rate limit summary notification"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        ?e,
                        api_key_id = %api_key_id,
                        "Failed to send rate limit notification"
                    );
                }
            }

            // Reset the counter after sending (so we don't send again for same hits)
            let _ = NotificationEventTracker::reset_rate_limit_hits(redis_pool, api_key_id).await;
        }
    }

    Ok(())
}

/// Process quota exceeded notifications
/// These are flagged in Redis when quota is exceeded, and we send them here
async fn process_quota_exceeded_notifications(
    redis_pool: &Arc<RedisPool>,
    pg_pool: &SqlxPgPool,
) -> Result<()> {
    // Scan for quota exceeded flags set today
    let date = NotificationEventTracker::today_date_str();
    let pattern = format!("vaultless:notify:quota_exceeded:*:{}", date);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let mut cursor: u64 = 0;

    loop {
        let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(&pattern)
            .arg("COUNT")
            .arg(100)
            .query_async(&mut *conn)
            .await
            .map_err(|e| VaultlessError::Internal(e.to_string()))?;

        for key in keys {
            // Extract UUID from key: vaultless:notify:quota_exceeded:{uuid}:{date}
            if let Some(uuid_str) = key.split(':').nth(3) {
                if let Ok(api_key_id) = Uuid::parse_str(uuid_str) {
                    // Check if notification was already sent (value is "sent" vs "1")
                    let value: Option<String> = conn
                        .get(&key)
                        .await
                        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

                    if value.as_deref() == Some("1") {
                        // Not yet sent, send now
                        if let Ok(Some((app_id, user_id, app_name))) =
                            Notification::get_app_info_for_notification(pg_pool, api_key_id).await
                        {
                            match Notification::send_quota_exceeded(pg_pool, user_id, &app_name, app_id)
                                .await
                            {
                                Ok(notification) => {
                                    tracing::info!(
                                        notification_id = %notification.id,
                                        user_id = %user_id,
                                        app_name = %app_name,
                                        "Sent quota exceeded notification"
                                    );

                                    // Mark as sent
                                    let _: std::result::Result<(), redis::RedisError> = conn
                                        .set(&key, "sent")
                                        .await;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        ?e,
                                        api_key_id = %api_key_id,
                                        "Failed to send quota exceeded notification"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_type_display() {
        assert_eq!(NotificationType::QuotaWarning.to_string(), "quota_warning");
        assert_eq!(NotificationType::SecurityAlert.to_string(), "security_alert");
    }

    #[test]
    fn test_notification_severity_display() {
        assert_eq!(NotificationSeverity::Info.to_string(), "info");
        assert_eq!(NotificationSeverity::Critical.to_string(), "critical");
    }

    #[test]
    fn test_notification_builder() {
        let user_id = Uuid::new_v4();
        let notification = NotificationBuilder::new(
            user_id,
            "Test Title",
            "Test Message",
            NotificationType::SystemUpdate,
        )
        .severity(NotificationSeverity::Warning)
        .action_url("/test")
        .metadata(serde_json::json!({"key": "value"}))
        .expires_in_days(7)
        .build();

        assert_eq!(notification.user_id, user_id);
        assert_eq!(notification.title, "Test Title");
        assert_eq!(notification.message, "Test Message");
        assert_eq!(notification.notification_type, NotificationType::SystemUpdate);
        assert_eq!(notification.severity, NotificationSeverity::Warning);
        assert_eq!(notification.action_url, Some("/test".to_string()));
        assert!(notification.metadata.is_some());
        assert!(notification.expires_at.is_some());
    }
}
