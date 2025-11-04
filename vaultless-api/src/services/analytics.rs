// vaultless-api/src/services/analytics.rs
use chrono::Datelike;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::middleware::error::ApiError;
use vaultless_core::{ApiKey, DailyUsageSummary, SubscriptionTier, UsageTrends};
/// Main analytics service with intelligent caching
pub struct AnalyticsService {
    db: Arc<PgPool>,
}

/// Complete analytics dashboard response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsDashboard {
    pub overview: UsageOverview,
    pub trends: UsageTrends,
    pub cost_breakdown: CostBreakdown,
    pub tier_info: TierInfo,
    pub quota_status: QuotaStatus,
    pub recent_activity: Vec<DailyUsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageOverview {
    pub total_messages_sent: i64,
    pub total_messages_received: i64,
    pub total_proofs_verified: i64,
    pub total_bytes_stored: i64,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_rate_limit_hits: i64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub messages_cost_cents: i64,
    pub storage_cost_cents: i64,
    pub verification_cost_cents: i64,
    pub total_cost_cents: i64,
    pub overage_cost_cents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierInfo {
    pub current_tier: SubscriptionTier,
    pub monthly_quota: i32,
    pub rate_limit_per_minute: i32,
    pub retention_days: i32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaStatus {
    pub messages_used: i64,
    pub messages_limit: i64,
    pub usage_percentage: f64,
    pub is_over_quota: bool,
    pub overage_count: i64,
    pub resets_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesDataPoint {
    pub timestamp: DateTime<Utc>,
    pub messages_sent: i64,
    pub messages_received: i64,
    pub proofs_verified: i64,
    pub bytes_stored: i64,
    pub bytes_sent: i64,
    pub bytes_received: i64,
}

impl AnalyticsService {
    pub fn new(db: Arc<PgPool>) -> Self {
        Self { db }
    }
    /// Get complete dashboard data for an API key
    pub async fn get_dashboard(
        &self,
        api_key_id: Uuid,
        user_tier: SubscriptionTier,
    ) -> Result<AnalyticsDashboard, ApiError> {
        // Fetch all data in parallel
        let (overview, trends, cost_breakdown, tier_info, quota_status, recent_activity) = tokio::join!(
            self.get_usage_overview(api_key_id),
            self.get_usage_trends(api_key_id),
            self.calculate_cost_breakdown(api_key_id, user_tier),
            self.get_tier_info(api_key_id),
            self.get_quota_status(api_key_id),
            self.get_recent_daily_activity(api_key_id, user_tier),
        );

        let dashboard = AnalyticsDashboard {
            overview: overview?,
            trends: trends?,
            cost_breakdown: cost_breakdown?,
            tier_info: tier_info?,
            quota_status: quota_status?,
            recent_activity: recent_activity?,
        };

        Ok(dashboard)
    }

    /// Get usage overview for current month
    async fn get_usage_overview(&self, api_key_id: Uuid) -> Result<UsageOverview, ApiError> {
        let now = Utc::now();
        let month_start = now
            .date_naive()
            .with_day(1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();

        let usage = vaultless_core::models::usage_timescale::get_realtime_usage(
            &self.db,
            api_key_id,
            month_start,
        )
        .await
        .map_err(|e| ApiError::internal_server_error(format!("Failed to fetch usage: {}", e)))?;

        Ok(UsageOverview {
            total_messages_sent: usage.total_messages_sent,
            total_messages_received: usage.total_messages_received,
            total_proofs_verified: usage.total_proofs_verified,
            total_bytes_stored: usage.total_bytes_stored,
            total_bytes_sent: usage.total_bytes_sent,
            total_bytes_received: usage.total_bytes_received,
            total_rate_limit_hits: usage.total_rate_limit_hits,
            period_start: month_start,
            period_end: now,
        })
    }

    /// Get usage trends (week-over-week comparison)
    async fn get_usage_trends(&self, api_key_id: Uuid) -> Result<UsageTrends, ApiError> {
        vaultless_core::models::usage_timescale::get_usage_trends(&self.db, api_key_id)
            .await
            .map_err(|e| ApiError::internal_server_error(format!("Failed to fetch trends: {}", e)))
    }

    /// Calculate cost breakdown with tier-specific pricing
    async fn calculate_cost_breakdown(
        &self,
        api_key_id: Uuid,
        tier: SubscriptionTier,
    ) -> Result<CostBreakdown, ApiError> {
        let usage = DailyUsageSummary::get_current_month_total(&self.db, api_key_id)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch usage: {}", e))
            })?;

        // Pricing model (cents per unit)
        let (msg_cost, storage_cost, verify_cost) = match tier {
            SubscriptionTier::Free => (0.0, 0.0, 0.0), // Free tier = $0
            SubscriptionTier::Starter => (0.001, 0.01, 0.0005), // $0.001/msg, $0.01/GB, $0.0005/verify
            SubscriptionTier::Pro => (0.0008, 0.008, 0.0004),
            SubscriptionTier::Enterprise => (0.0005, 0.005, 0.0002),
        };

        let messages_cost = (usage.total_messages_sent as f64 * msg_cost) as i64;
        let storage_cost_val =
            ((usage.total_bytes_stored as f64 / 1_073_741_824.0) * storage_cost) as i64;
        let verification_cost = (usage.total_proofs_verified as f64 * verify_cost) as i64;

        let total_cost = messages_cost + storage_cost_val + verification_cost;

        // Calculate overage cost
        let api_key = ApiKey::find_by_id(self.db.as_ref(), None, api_key_id)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch API key: {}", e))
            })?;

        let overage = usage.total_messages_sent - api_key.monthly_message_quota as i64;
        let overage_cost = if overage > 0 {
            (overage as f64 * 0.01) as i64 // $0.01 per overage message
        } else {
            0
        };

        Ok(CostBreakdown {
            messages_cost_cents: messages_cost,
            storage_cost_cents: storage_cost_val,
            verification_cost_cents: verification_cost,
            total_cost_cents: total_cost,
            overage_cost_cents: overage_cost,
        })
    }

    /// Get tier information
    async fn get_tier_info(&self, api_key_id: Uuid) -> Result<TierInfo, ApiError> {
        let api_key = ApiKey::find_by_id(self.db.as_ref(), None, api_key_id)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch API key: {}", e))
            })?;

        let features = self.get_tier_features(&api_key.tier);
        let retention_days = api_key.message_retention_seconds / 86400;

        Ok(TierInfo {
            current_tier: api_key.tier,
            monthly_quota: api_key.monthly_message_quota,
            rate_limit_per_minute: api_key.rate_limit_per_minute,
            retention_days,
            features,
        })
    }

    /// Get quota status with percentage calculation
    pub async fn get_quota_status(&self, api_key_id: Uuid) -> Result<QuotaStatus, ApiError> {
        let api_key = ApiKey::find_by_id(self.db.as_ref(), None, api_key_id)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch API key: {}", e))
            })?;

        let usage = DailyUsageSummary::get_current_month_total(&self.db, api_key_id)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch usage: {}", e))
            })?;

        let messages_used = usage.total_messages_sent;
        let messages_limit = api_key.monthly_message_quota as i64;
        let usage_percentage = (messages_used as f64 / messages_limit as f64) * 100.0;
        let is_over_quota = messages_used > messages_limit;
        let overage_count = if is_over_quota {
            messages_used - messages_limit
        } else {
            0
        };

        // Calculate when quota resets (first of next month)
        let now = Utc::now();
        let next_month = if now.month() == 12 {
            now.date_naive()
                .with_year(now.year() + 1)
                .unwrap()
                .with_month(1)
                .unwrap()
        } else {
            now.date_naive().with_month(now.month() + 1).unwrap()
        };
        let resets_at = next_month
            .with_day(1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();

        Ok(QuotaStatus {
            messages_used,
            messages_limit,
            usage_percentage,
            is_over_quota,
            overage_count,
            resets_at,
        })
    }

    /// Get recent daily activity (tier-limited)
    async fn get_recent_daily_activity(
        &self,
        api_key_id: Uuid,
        tier: SubscriptionTier,
    ) -> Result<Vec<DailyUsageSummary>, ApiError> {
        // Tier-based historical data access
        let days = match tier {
            SubscriptionTier::Free => 0,         // No historical data
            SubscriptionTier::Starter => 7,      // Last 7 days
            SubscriptionTier::Pro => 90,         // Last 90 days
            SubscriptionTier::Enterprise => 365, // Last year
        };

        if days == 0 {
            return Ok(vec![]);
        }

        DailyUsageSummary::get_last_n_days(&self.db, api_key_id, days)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch daily usage: {}", e))
            })
    }

    /// Get time series data for charts (tier-limited)
    pub async fn get_time_series(
        &self,
        api_key_id: Uuid,
        tier: SubscriptionTier,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TimeSeriesDataPoint>, ApiError> {
        // Enforce tier limits on historical data
        let max_days = match tier {
            SubscriptionTier::Free => {
                return Err(ApiError::forbidden("Upgrade to access historical data"));
            }
            SubscriptionTier::Starter => 7,
            SubscriptionTier::Pro => 90,
            SubscriptionTier::Enterprise => 365,
        };

        let requested_duration = end.signed_duration_since(start).num_days();
        if requested_duration > max_days {
            return Err(ApiError::forbidden(format!(
                "Your tier allows {} days of historical data. Upgrade for more.",
                max_days
            )));
        }

        let daily_summaries = DailyUsageSummary::get_range(&self.db, api_key_id, start, end)
            .await
            .map_err(|e| {
                ApiError::internal_server_error(format!("Failed to fetch time series: {}", e))
            })?;

        let data_points = daily_summaries
            .into_iter()
            .map(|s| {
                let sent = s.total_bytes_sent.unwrap_or(0);
                let received = s.total_bytes_received.unwrap_or(0);
                TimeSeriesDataPoint {
                    timestamp: s.day,
                    messages_sent: s.total_messages_sent.unwrap_or(0),
                    messages_received: s.total_messages_received.unwrap_or(0),
                    proofs_verified: s.total_proofs_verified.unwrap_or(0),
                    bytes_stored: s.total_bytes_stored.unwrap_or(0),
                    bytes_sent: sent,
                    bytes_received: received,
                }
            })
            .collect();

        Ok(data_points)
    }

    /// Get tier features list
    fn get_tier_features(&self, tier: &SubscriptionTier) -> Vec<String> {
        match tier {
            SubscriptionTier::Free => vec![
                "1,000 messages/month".to_string(),
                "7-day retention".to_string(),
                "60 req/min rate limit".to_string(),
                "Community support".to_string(),
            ],
            SubscriptionTier::Starter => vec![
                "50,000 messages/month".to_string(),
                "30-day retention".to_string(),
                "300 req/min rate limit".to_string(),
                "7-day analytics".to_string(),
                "Email support".to_string(),
            ],
            SubscriptionTier::Pro => vec![
                "500,000 messages/month".to_string(),
                "90-day retention".to_string(),
                "1,000 req/min rate limit".to_string(),
                "90-day analytics".to_string(),
                "Real-time webhooks".to_string(),
                "Priority support".to_string(),
            ],
            SubscriptionTier::Enterprise => vec![
                "Unlimited messages".to_string(),
                "Custom retention".to_string(),
                "Unlimited rate limit".to_string(),
                "Full analytics history".to_string(),
                "Custom integrations".to_string(),
                "SLA guarantees".to_string(),
                "Dedicated support".to_string(),
            ],
        }
    }

    /// Check if user should be notified about quota
    pub async fn check_quota_alerts(
        &self,
        api_key_id: Uuid,
    ) -> Result<Option<QuotaAlert>, ApiError> {
        let quota_status = self.get_quota_status(api_key_id).await?;

        let alert_type = if quota_status.is_over_quota {
            Some(QuotaAlertType::OverQuota)
        } else if quota_status.usage_percentage >= 90.0 {
            Some(QuotaAlertType::Critical) // 90%+
        } else if quota_status.usage_percentage >= 80.0 {
            Some(QuotaAlertType::Warning) // 80%+
        } else {
            None
        };

        Ok(alert_type.map(|alert_type| QuotaAlert {
            api_key_id,
            alert_type,
            usage_percentage: quota_status.usage_percentage,
            messages_used: quota_status.messages_used,
            messages_limit: quota_status.messages_limit,
            overage_count: quota_status.overage_count,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaAlert {
    pub api_key_id: Uuid,
    pub alert_type: QuotaAlertType,
    pub usage_percentage: f64,
    pub messages_used: i64,
    pub messages_limit: i64,
    pub overage_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaAlertType {
    Warning,   // 80% usage
    Critical,  // 90% usage
    OverQuota, // Exceeded quota
}
