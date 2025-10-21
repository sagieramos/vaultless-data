use chrono::{DateTime, Datelike, Timelike, Utc};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

use crate::services::analytics::AnalyticsService;
use crate::services::cache::CacheService;
use vaultless_core::{ApiKey, Notification, NotificationBuilder};

/// Notification automation service
/// Handles quota alerts, expiry warnings, and scheduled notifications
pub struct NotificationService {
    db: PgPool,
    analytics: AnalyticsService,
}

// Define the required thread-safe error type for convenience in this file
// This is a common pattern when using Box<dyn Error> in async code.
type StdError = Box<dyn std::error::Error + Send + Sync + 'static>;

impl NotificationService {
    pub fn new(db: PgPool, cache: CacheService) -> Self {
        let analytics = AnalyticsService::new(db.clone(), cache);
        Self { db, analytics }
    }

    /// Run all periodic notification checks
    /// This should be called by a cron job or background worker
    pub async fn run_periodic_checks(&self) -> Result<(), StdError> {
        tracing::info!("Running periodic notification checks...");

        // Run all checks in parallel
        let (quota_result, expiry_result, cleanup_result) = tokio::join!(
            self.check_quota_alerts(),
            self.check_api_key_expiry(),
            self.cleanup_old_notifications(),
        );

        quota_result?;
        expiry_result?;
        cleanup_result?;

        tracing::info!("Periodic notification checks completed");
        Ok(())
    }

    /// Check quota usage and send alerts at 80%, 90%, and 100%
    async fn check_quota_alerts(&self) -> Result<(), StdError> {
        tracing::info!("Checking quota alerts...");

        // Get all active API keys
        let api_keys = sqlx::query_as::<_, ApiKey>(
            r#"
            SELECT * FROM api_keys 
            WHERE is_active = true 
                AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        let mut alerts_sent = 0;

        for api_key in api_keys {
            if let Some(alert) = self.analytics.check_quota_alerts(api_key.id).await? {
                // Check if we've already sent this alert recently (use cache)
                let cache_key = format!("quota_alert:{}:{:?}", api_key.id, alert.alert_type);

                // Skip if alert was sent in last 24 hours
                if self.was_alert_sent_recently(&cache_key).await {
                    continue;
                }

                match alert.alert_type {
                    crate::services::analytics::QuotaAlertType::Warning => {
                        NotificationBuilder::quota_warning(
                            &self.db,
                            api_key.user_id,
                            alert.usage_percentage,
                            alert.messages_used,
                            alert.messages_limit,
                        )
                        .await?;
                        alerts_sent += 1;
                    }
                    crate::services::analytics::QuotaAlertType::Critical => {
                        NotificationBuilder::quota_warning(
                            &self.db,
                            api_key.user_id,
                            alert.usage_percentage,
                            alert.messages_used,
                            alert.messages_limit,
                        )
                        .await?;
                        alerts_sent += 1;
                    }
                    crate::services::analytics::QuotaAlertType::OverQuota => {
                        NotificationBuilder::quota_exceeded(
                            &self.db,
                            api_key.user_id,
                            alert.overage_count,
                            (alert.overage_count as f64 * 0.01 * 100.0) as i64, // $0.01 per message
                        )
                        .await?;
                        alerts_sent += 1;
                    }
                }

                // Mark alert as sent (cache for 24 hours)
                self.mark_alert_sent(&cache_key).await;
            }
        }

        tracing::info!("Sent {} quota alerts", alerts_sent);
        Ok(())
    }

    /// Check for API keys expiring in 7, 3, and 1 days
    async fn check_api_key_expiry(&self) -> Result<(), StdError> {
        tracing::info!("Checking API key expiry...");

        let expiry_thresholds = vec![7, 3, 1]; // Days before expiry
        let mut alerts_sent = 0;

        for days in expiry_thresholds {
            let expiry_date = Utc::now() + chrono::Duration::days(days);

            let expiring_keys = sqlx::query_as::<_, ApiKey>(
                r#"
                SELECT * FROM api_keys 
                WHERE is_active = true 
                    AND expires_at IS NOT NULL
                    AND expires_at > NOW()
                    AND expires_at <= $1
                "#,
            )
            .bind(expiry_date)
            .fetch_all(&self.db)
            .await?;

            for api_key in expiring_keys {
                let cache_key = format!("expiry_alert:{}:{}", api_key.id, days);

                if self.was_alert_sent_recently(&cache_key).await {
                    continue;
                }

                NotificationBuilder::api_key_expiring(
                    &self.db,
                    api_key.user_id,
                    api_key.key_prefix.clone(),
                    api_key.expires_at.unwrap(),
                )
                .await?;

                self.mark_alert_sent(&cache_key).await;
                alerts_sent += 1;
            }
        }

        tracing::info!("Sent {} API key expiry alerts", alerts_sent);
        Ok(())
    }

    /// Clean up old notifications (expired and old read notifications)
    async fn cleanup_old_notifications(&self) -> Result<(), StdError> {
        tracing::info!("Cleaning up old notifications...");

        // Delete expired notifications
        let expired_count = Notification::cleanup_expired(&self.db).await?;
        tracing::info!("Deleted {} expired notifications", expired_count);

        // Delete read notifications older than 90 days
        let old_read_count = Notification::cleanup_old_read(&self.db, 90).await?;
        tracing::info!("Deleted {} old read notifications", old_read_count);

        Ok(())
    }

    /// Send monthly usage reports (should run on 1st of each month)
    pub async fn send_monthly_usage_reports(&self) -> Result<(), StdError> {
        tracing::info!("Sending monthly usage reports...");

        // Get all active users with API keys
        let users = sqlx::query_as::<_, (Uuid,)>(
            r#"
            SELECT DISTINCT user_id FROM api_keys 
            WHERE is_active = true
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        let now = Utc::now();
        let month_name = format!(
            "{} {}",
            match now.month() - 1 {
                1 => "January",
                2 => "February",
                3 => "March",
                4 => "April",
                5 => "May",
                6 => "June",
                7 => "July",
                8 => "August",
                9 => "September",
                10 => "October",
                11 => "November",
                12 => "December",
                _ => "December",
            },
            now.year()
        );

        let mut reports_sent = 0;

        for (user_id,) in users {
            // Get user's API keys
            let api_keys = ApiKey::find_by_owner(&self.db, user_id).await?;

            if api_keys.is_empty() {
                continue;
            }

            // Aggregate usage across all API keys
            let mut total_messages_sent = 0i64;
            let mut total_cost_cents = 0i64;

            for api_key in api_keys {
                let dashboard = self
                    .analytics
                    .get_dashboard(api_key.id, api_key.tier)
                    .await?;
                total_messages_sent += dashboard.overview.total_messages_sent;
                total_cost_cents += dashboard.cost_breakdown.total_cost_cents;
            }

            // Send report notification
            NotificationBuilder::monthly_usage_report(
                &self.db,
                user_id,
                total_messages_sent,
                total_cost_cents,
                month_name.clone(),
            )
            .await?;

            reports_sent += 1;
        }

        tracing::info!("Sent {} monthly usage reports", reports_sent);
        Ok(())
    }

    /// Broadcast system update to all users
    pub async fn broadcast_system_update(
        &self,
        title: String,
        message: String,
        action_url: Option<String>,
    ) -> Result<u64, StdError> {
        tracing::info!("Broadcasting system update: {}", title);

        let users = sqlx::query_as::<_, (Uuid,)>(
            r#"
            SELECT DISTINCT user_id FROM api_keys 
            WHERE is_active = true
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        let mut sent_count = 0u64;

        for (user_id,) in users {
            NotificationBuilder::system_update(
                &self.db,
                user_id,
                title.clone(),
                message.clone(),
                action_url.clone(),
            )
            .await?;
            sent_count += 1;
        }

        tracing::info!("Broadcast sent to {} users", sent_count);
        Ok(sent_count)
    }

    /// Send targeted promotional offer to specific tier users
    pub async fn send_promotional_offer(
        &self,
        target_tier: vaultless_core::SubscriptionTier,
        offer_title: String,
        offer_details: String,
        promo_code: Option<String>,
        expires_at: DateTime<Utc>,
    ) -> Result<u64, StdError> {
        tracing::info!("Sending promotional offer to {:?} tier users", target_tier);

        let users = sqlx::query_as::<_, (Uuid,)>(
            r#"
            SELECT DISTINCT user_id FROM api_keys 
            WHERE is_active = true AND tier = $1
            "#,
        )
        .bind(target_tier)
        .fetch_all(&self.db)
        .await?;

        let mut sent_count = 0u64;

        for (user_id,) in users {
            NotificationBuilder::promotional_offer(
                &self.db,
                user_id,
                offer_title.clone(),
                offer_details.clone(),
                promo_code.clone(),
                expires_at,
            )
            .await?;
            sent_count += 1;
        }

        tracing::info!("Promotional offer sent to {} users", sent_count);
        Ok(sent_count)
    }

    /// Helper: Check if alert was sent recently (via cache)
    async fn was_alert_sent_recently(&self, cache_key: &str) -> bool {
        // In production, check Redis/Dragonfly cache
        // For now, return false to always send
        false
    }

    /// Helper: Mark alert as sent (cache for 24 hours)
    async fn mark_alert_sent(&self, cache_key: &str) {
        // In production, set in Redis/Dragonfly with 24h TTL
        tracing::debug!("Marked alert as sent: {}", cache_key);
    }
}

/// Background worker that runs notification checks periodically
pub async fn notification_worker(db: PgPool, cache: CacheService) {
    let service = NotificationService::new(db, cache);
    let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Run every hour

    loop {
        interval.tick().await;

        if let Err(e) = service.run_periodic_checks().await {
            tracing::error!("Notification worker error: {}", e);
        }
    }
}

/// Monthly report worker (runs on 1st of each month)
pub async fn monthly_report_worker(db: PgPool, cache: CacheService) {
    let service = NotificationService::new(db, cache);

    loop {
        let now = Utc::now();

        // Check if it's the 1st day of the month at midnight
        if now.day() == 1 && now.hour() == 0 {
            if let Err(e) = service.send_monthly_usage_reports().await {
                tracing::error!("Monthly report worker error: {}", e);
            }

            // Sleep for 24 hours to avoid sending multiple times
            tokio::time::sleep(Duration::from_secs(86400)).await;
        } else {
            // Check every hour
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    }
}
