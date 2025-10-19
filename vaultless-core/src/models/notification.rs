// vaultless-core/src/models/notification.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub message: String,
    pub notification_type: NotificationType,
    pub severity: NotificationSeverity,
    pub is_read: bool,
    pub action_url: Option<String>, // Deep link for "View Details" or "Upgrade Now"
    pub metadata: Option<sqlx::types::JsonValue>, // Extra context data
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>, // Auto-delete after expiry
}

/// Notification types for categorization and filtering
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "notification_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    QuotaWarning,   // 80% quota usage
    QuotaExceeded,  // Over quota
    BillingAlert,   // Payment issues
    SecurityAlert,  // Suspicious activity
    SystemUpdate,   // Maintenance or new features
    MarketingOffer, // Promotional offers
    ApiKeyExpiring, // API key expiring soon
    UsageReport,    // Monthly usage summary
}

/// Severity levels for UI prioritization
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, PartialOrd, Ord,
)]
#[sqlx(type_name = "notification_severity", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum NotificationSeverity {
    Info,
    Warning,
    Critical,
}

/// Query parameters for listing notifications
#[derive(Debug, Clone, Deserialize)]
pub struct NotificationFilters {
    pub notification_type: Option<NotificationType>,
    pub severity: Option<NotificationSeverity>,
    pub is_read: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for NotificationFilters {
    fn default() -> Self {
        Self {
            notification_type: None,
            severity: None,
            is_read: None,
            limit: Some(20),
            offset: Some(0),
        }
    }
}

impl Notification {
    /// Create a new notification (system-only, not exposed to users)
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        title: String,
        message: String,
        notification_type: NotificationType,
        severity: NotificationSeverity,
        action_url: Option<String>,
        metadata: Option<serde_json::Value>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Self> {
        let notification = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO notifications (
                user_id, title, message, notification_type, 
                severity, action_url, metadata, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(title)
        .bind(message)
        .bind(notification_type)
        .bind(severity)
        .bind(action_url)
        .bind(metadata)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(notification)
    }

    /// Get a single notification by ID (with ownership check)
    pub async fn find_by_id(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<Self> {
        let notification = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM notifications 
            WHERE id = $1 AND user_id = $2
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Notification not found".to_string()))?;

        Ok(notification)
    }

    /// List notifications for a user with filters
    pub async fn list(
        pool: &PgPool,
        user_id: Uuid,
        filters: NotificationFilters,
    ) -> Result<Vec<Self>> {
        let limit = filters.limit.unwrap_or(20).min(100); // Max 100 per page
        let offset = filters.offset.unwrap_or(0);

        let mut query = String::from(
            r#"
            SELECT * FROM notifications 
            WHERE user_id = $1
            AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        );

        let mut param_count = 1;

        // Build dynamic query based on filters
        if filters.notification_type.is_some() {
            param_count += 1;
            query.push_str(&format!(" AND notification_type = ${}", param_count));
        }

        if filters.severity.is_some() {
            param_count += 1;
            query.push_str(&format!(" AND severity = ${}", param_count));
        }

        if let Some(is_read) = filters.is_read {
            param_count += 1;
            query.push_str(&format!(" AND is_read = ${}", param_count));
        }

        query.push_str(" ORDER BY created_at DESC");
        query.push_str(&format!(
            " LIMIT ${} OFFSET ${}",
            param_count + 1,
            param_count + 2
        ));

        let mut query_builder = sqlx::query_as::<_, Self>(&query).bind(user_id);

        if let Some(ntype) = filters.notification_type {
            query_builder = query_builder.bind(ntype);
        }

        if let Some(sev) = filters.severity {
            query_builder = query_builder.bind(sev);
        }

        if let Some(is_read) = filters.is_read {
            query_builder = query_builder.bind(is_read);
        }

        let notifications = query_builder
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?;

        Ok(notifications)
    }

    /// Mark notification as read
    pub async fn mark_as_read(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<Self> {
        let notification = sqlx::query_as::<_, Self>(
            r#"
            UPDATE notifications 
            SET is_read = true, read_at = NOW()
            WHERE id = $1 AND user_id = $2
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Notification not found".to_string()))?;

        Ok(notification)
    }

    /// Mark all notifications as read for a user
    pub async fn mark_all_as_read(pool: &PgPool, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE notifications 
            SET is_read = true, read_at = NOW()
            WHERE user_id = $1 AND is_read = false
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete a notification (hard delete)
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

        Ok(())
    }

    /// Delete all read notifications for a user
    pub async fn delete_all_read(pool: &PgPool, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications 
            WHERE user_id = $1 AND is_read = true
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get unread count for a user
    pub async fn get_unread_count(pool: &PgPool, user_id: Uuid) -> Result<i64> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM notifications 
            WHERE user_id = $1 
                AND is_read = false
                AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(count)
    }

    /// Get notification statistics for a user
    pub async fn get_stats(pool: &PgPool, user_id: Uuid) -> Result<NotificationStats> {
        let stats = sqlx::query_as::<_, NotificationStats>(
            r#"
            SELECT 
                COUNT(*) as total,
                COUNT(*) FILTER (WHERE is_read = false) as unread,
                COUNT(*) FILTER (WHERE severity = 'critical') as critical,
                COUNT(*) FILTER (WHERE severity = 'warning') as warnings,
                COUNT(*) FILTER (WHERE created_at > NOW() - INTERVAL '24 hours') as last_24h
            FROM notifications
            WHERE user_id = $1
                AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(stats)
    }

    /// Clean up expired notifications (background job)
    pub async fn cleanup_expired(pool: &PgPool) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications 
            WHERE expires_at IS NOT NULL 
                AND expires_at < NOW()
            "#,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Clean up old read notifications (retention policy)
    pub async fn cleanup_old_read(pool: &PgPool, retention_days: i32) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM notifications 
            WHERE is_read = true 
                AND read_at < NOW() - $1::INTERVAL
            "#,
        )
        .bind(format!("{} days", retention_days))
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct NotificationStats {
    pub total: i64,
    pub unread: i64,
    pub critical: i64,
    pub warnings: i64,
    pub last_24h: i64,
}

// ============================================================================
// NOTIFICATION BUILDERS (for system-generated notifications)
// ============================================================================

pub struct NotificationBuilder;

impl NotificationBuilder {
    /// Create quota warning notification (80% usage)
    pub async fn quota_warning(
        pool: &PgPool,
        user_id: Uuid,
        usage_percentage: f64,
        messages_used: i64,
        messages_limit: i64,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            "Quota Warning".to_string(),
            format!(
                "You've used {:.1}% ({}/{}) of your monthly message quota. Consider upgrading to avoid service interruption.",
                usage_percentage, messages_used, messages_limit
            ),
            NotificationType::QuotaWarning,
            NotificationSeverity::Warning,
            Some("/dashboard/usage".to_string()),
            Some(serde_json::json!({
                "usage_percentage": usage_percentage,
                "messages_used": messages_used,
                "messages_limit": messages_limit
            })),
            Some(Utc::now() + chrono::Duration::days(7)), // Expire in 7 days
        )
        .await
    }

    /// Create quota exceeded notification
    pub async fn quota_exceeded(
        pool: &PgPool,
        user_id: Uuid,
        overage_count: i64,
        overage_cost_cents: i64,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            "⚠️ Quota Exceeded".to_string(),
            format!(
                "You've exceeded your monthly quota by {} messages. Overage charges: ${:.2}. Upgrade to avoid future charges.",
                overage_count,
                overage_cost_cents as f64 / 100.0
            ),
            NotificationType::QuotaExceeded,
            NotificationSeverity::Critical,
            Some("/dashboard/billing".to_string()),
            Some(serde_json::json!({
                "overage_count": overage_count,
                "overage_cost_cents": overage_cost_cents
            })),
            None, // Don't expire critical notifications
        )
        .await
    }

    /// Create API key expiring notification
    pub async fn api_key_expiring(
        pool: &PgPool,
        user_id: Uuid,
        api_key_prefix: String,
        expires_at: DateTime<Utc>,
    ) -> Result<Notification> {
        let days_until_expiry = (expires_at - Utc::now()).num_days();

        Notification::create(
            pool,
            user_id,
            "API Key Expiring Soon".to_string(),
            format!(
                "Your API key '{}...' will expire in {} days. Renew it to avoid service disruption.",
                api_key_prefix, days_until_expiry
            ),
            NotificationType::ApiKeyExpiring,
            NotificationSeverity::Warning,
            Some("/dashboard/keys".to_string()),
            Some(serde_json::json!({
                "api_key_prefix": api_key_prefix,
                "expires_at": expires_at,
                "days_until_expiry": days_until_expiry
            })),
            Some(expires_at), // Expire when key expires
        )
        .await
    }

    /// Create payment failed notification
    pub async fn payment_failed(
        pool: &PgPool,
        user_id: Uuid,
        invoice_amount_cents: i64,
        attempt_count: i32,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            "🚨 Payment Failed".to_string(),
            format!(
                "We couldn't process your payment of ${:.2}. Please update your payment method. Attempt {}/3.",
                invoice_amount_cents as f64 / 100.0,
                attempt_count
            ),
            NotificationType::BillingAlert,
            NotificationSeverity::Critical,
            Some("/dashboard/billing".to_string()),
            Some(serde_json::json!({
                "invoice_amount_cents": invoice_amount_cents,
                "attempt_count": attempt_count
            })),
            None,
        )
        .await
    }

    /// Create monthly usage report notification
    pub async fn monthly_usage_report(
        pool: &PgPool,
        user_id: Uuid,
        messages_sent: i64,
        total_cost_cents: i64,
        month: String,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            format!("📊 {} Usage Report", month),
            format!(
                "Your monthly summary: {} messages sent, total cost: ${:.2}. View detailed analytics.",
                messages_sent,
                total_cost_cents as f64 / 100.0
            ),
            NotificationType::UsageReport,
            NotificationSeverity::Info,
            Some("/analytics/dashboard".to_string()),
            Some(serde_json::json!({
                "messages_sent": messages_sent,
                "total_cost_cents": total_cost_cents,
                "month": month
            })),
            Some(Utc::now() + chrono::Duration::days(90)), // Expire in 90 days
        )
        .await
    }

    /// Create security alert notification
    pub async fn security_alert(
        pool: &PgPool,
        user_id: Uuid,
        alert_type: String,
        details: String,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            format!("🔐 Security Alert: {}", alert_type),
            details,
            NotificationType::SecurityAlert,
            NotificationSeverity::Critical,
            Some("/dashboard/security".to_string()),
            Some(serde_json::json!({
                "alert_type": alert_type,
                "timestamp": Utc::now()
            })),
            None,
        )
        .await
    }

    /// Create system update notification
    pub async fn system_update(
        pool: &PgPool,
        user_id: Uuid,
        title: String,
        message: String,
        action_url: Option<String>,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            title,
            message,
            NotificationType::SystemUpdate,
            NotificationSeverity::Info,
            action_url,
            None,
            Some(Utc::now() + chrono::Duration::days(30)),
        )
        .await
    }

    /// Create promotional offer notification
    pub async fn promotional_offer(
        pool: &PgPool,
        user_id: Uuid,
        offer_title: String,
        offer_details: String,
        promo_code: Option<String>,
        expires_at: DateTime<Utc>,
    ) -> Result<Notification> {
        Notification::create(
            pool,
            user_id,
            offer_title,
            offer_details,
            NotificationType::MarketingOffer,
            NotificationSeverity::Info,
            Some("/dashboard/upgrade".to_string()),
            Some(serde_json::json!({
                "promo_code": promo_code,
                "expires_at": expires_at
            })),
            Some(expires_at),
        )
        .await
    }
}
