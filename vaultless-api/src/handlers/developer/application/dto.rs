//! Request and Response DTOs for application handlers.

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use vaultless_core::models::{
    Application,
    applications::{dto::*, integrity::dto::IntegrityConfig},
    usage::MetricCounters,
};

// =============================================================================
// Application DTOs
// =============================================================================

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyRevenueResponse {
    pub labels: Vec<String>,
    pub datasets: Vec<RevenueDataset>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevenueDataset {
    pub label: String,
    pub data: Vec<i64>,
    pub background_color: String,
    pub border_color: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyRevenueByApplicationResponse {
    pub application_id: Uuid,
    pub application_name: String,
    pub month: String,
    pub revenue_cents: i64,
    pub messages: i64,
    pub bytes_transferred: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationResponse {
    /// Application unique identifier
    #[schema(value_type = String)]
    pub id: Uuid,
    /// Application name
    pub name: String,
    /// Application description
    pub description: Option<String>,
    /// Whether the application is active
    pub is_active: bool,
    /// Creation timestamp
    #[schema(value_type = String)]
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    #[schema(value_type = String)]
    pub updated_at: DateTime<Utc>,
    /// Maximum time-to-live in seconds
    pub max_ttl_seconds: i32,
    /// Whether key rotation is forced
    pub is_key_rotation_forced: bool,
    /// Deletion requested timestamp (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>)]
    pub deletion_requested_at: Option<DateTime<Utc>>,
    /// Internal notes about the application
    pub internal_notes: Option<String>,
    /// Integrity configuration metadata
    pub integrity_config: IntegrityConfig,
}

impl From<Application> for ApplicationResponse {
    fn from(app: Application) -> Self {
        Self {
            id: app.id,
            name: app.name,
            description: app.description,
            is_active: app.is_active,
            created_at: app.created_at,
            updated_at: app.updated_at,
            max_ttl_seconds: app.max_ttl_seconds,
            is_key_rotation_forced: app.is_key_rotation_forced,
            deletion_requested_at: app.deletion_requested_at,
            internal_notes: app.internal_notes,
            integrity_config: app.app_meta.0.integrity_config,
        }
    }
}

/// Response returned when creating a new application
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationResponse {
    /// The created application details
    pub application: ApplicationResponse,
    /// Secret API key (only shown once - save immediately!)
    #[schema(example = "sk_live_abc123xyz...")]
    pub secret_key: String,
    /// Publishable API key
    #[schema(example = "pk_live_def456uvw...")]
    pub publishable_key: String,
    /// Important message about saving the secret key
    #[schema(example = "IMPORTANT: Save your secret key now. You won't be able to see it again!")]
    pub message: String,
}

// =============================================================================
// Pagination DTOs
// =============================================================================

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginationParams {
    /// Page number (default: 1)
    #[schema(example = 1)]
    pub page: Option<i64>,
    /// Page size (default: 20)
    #[schema(example = 20)]
    pub page_size: Option<i64>,
    /// Search applications by name
    #[schema(example = "My App")]
    pub search: Option<String>,
    /// Sort field: name, createdAt, updatedAt, quotaUsage
    #[schema(example = "createdAt")]
    pub sort: Option<String>,
    /// Sort order: asc or desc
    #[schema(example = "desc")]
    pub sort_order: Option<String>,
    /// Filter by active status
    #[schema(example = true)]
    pub filter_active: Option<bool>,
    /// Filter by inactive status
    #[schema(example = false)]
    pub filter_inactive: Option<bool>,
    /// Filter by tier (free, pro, enterprise)
    #[schema(example = "pro")]
    pub tier: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWarningsQuery {
    /// Percentage threshold (default: 80.0)
    #[serde(default = "default_threshold_f64")]
    #[schema(example = 80.0)]
    pub threshold: Option<f64>,
    /// Page number (default: 1)
    #[serde(default = "default_page")]
    #[schema(example = 1)]
    pub page: i64,
    /// Items per page (default: 20)
    #[serde(default = "default_page_size")]
    #[schema(example = 20)]
    pub page_size: i64,
    /// Type of quota to check: "messages" or "bandwidth"
    #[schema(example = "messages")]
    pub r#type: Option<String>,
}

fn default_threshold_f64() -> Option<f64> {
    Some(80.0)
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    20
}

// =============================================================================
// Chart DTOs
// =============================================================================

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChartQueryParams {
    #[schema(example = "daily")]
    pub granularity: String,
    #[schema(example = "messages")]
    pub metric: String,
    #[schema(example = "2023-01-01")]
    pub start: String,
    #[schema(example = "2023-01-31")]
    pub end: String,
}

// =============================================================================
// Real-time Usage DTOs
// =============================================================================

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RealTimeUsageResponse {
    #[schema(example = "2023-01-01T00:00:00Z")]
    pub current_period_start_utc: String,
    #[serde(flatten)]
    pub counters: MetricCounters,
}

// =============================================================================
// Dashboard Response DTOs
// =============================================================================

/// Quota status for an application
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaStatus {
    /// Messages used this month
    pub messages_used: i64,
    /// Messages limit (None if unlimited)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_limit: Option<i64>,
    /// Usage percentage (0-100+)
    pub usage_pct: f64,
    /// Whether the application is over quota
    pub is_over_quota: bool,
    /// Number of messages over quota
    pub overage_count: i64,
    /// When the quota resets
    #[schema(value_type = String)]
    pub resets_at: DateTime<Utc>,
    /// Alert level: "info", "warning", "critical", or None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert_level: Option<String>,
}

/// Usage trends for an application
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageTrends {
    /// Daily average messages this month
    pub daily_avg_messages: i64,
    /// Projected monthly messages based on current rate
    pub projected_monthly_messages: i64,
    /// Quota trend: "stable", "increasing", "critical"
    pub quota_trend: String,
}

/// Full application response with usage statistics for dashboards
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDashboardResponse {
    /// Application unique identifier
    #[schema(value_type = String)]
    pub id: Uuid,
    /// Application name
    pub name: String,
    /// Application description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// Whether the application is active
    pub active: bool,
    /// Creation timestamp
    #[schema(value_type = String)]
    pub created: DateTime<Utc>,
    /// Last update timestamp
    #[schema(value_type = String)]
    pub updated: DateTime<Utc>,
    /// Application metadata
    pub meta: vaultless_core::models::applications::integrity::dto::AppMetaData,

    // Subscription tier info
    /// Subscription tier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Monthly message quota
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_quota: Option<i64>,
    /// Rate limit per minute
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<i32>,
    /// Message retention in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention_seconds: Option<i64>,

    /// List of publishable keys
    #[schema(value_type = Vec<Object>)]
    pub keys: Vec<PublishableKey>,
    /// List of webhooks
    #[schema(value_type = Vec<Object>)]
    pub webhooks: Vec<Webhook>,

    /// Quota usage percentage
    pub quota_usage_pct: f64,

    /// Current month usage statistics
    pub current_month: UsageStats,
    /// Lifetime usage statistics
    pub lifetime: LifetimeStats,

    /// Attached pricing plan information (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_plan: Option<vaultless_core::models::applications::material_view::AttachedPricingPlan>,

    /// Real-time quota status
    pub quota_status: QuotaStatus,
    /// Usage trends
    pub trends: UsageTrends,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    /// Messages sent
    pub msg_sent: i64,
    /// Messages received
    pub msg_received: i64,
    /// Messages with proof verification
    pub msg_proof: i64,
    /// Bytes stored
    pub msg_stored: i64,
    /// Bytes sent
    pub bytes_sent: i64,
    /// Bytes received
    pub bytes_received: i64,
    /// Rate limit hits
    pub rate_hits: i64,
    /// Cost in cents
    pub cost: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LifetimeStats {
    /// Lifetime messages sent
    pub msg_sent: i64,
    /// Lifetime cost in cents
    pub cost: i64,
}

impl From<(ApplicationWithUsage, Option<vaultless_core::models::applications::material_view::AttachedPricingPlan>)> for ApplicationDashboardResponse {
    fn from((app, pricing_plan): (ApplicationWithUsage, Option<vaultless_core::models::applications::material_view::AttachedPricingPlan>)) -> Self {
        let usage_pct = app
            .quota_usage_percentage
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0);

        // Compute quota status
        let messages_limit = app.monthly_message_quota.unwrap_or(0);
        let is_over_quota =
            messages_limit > 0 && app.current_month_messages_sent > messages_limit;
        let overage_count = if is_over_quota {
            app.current_month_messages_sent - messages_limit
        } else {
            0
        };

        // Calculate reset time (first day of next month)
        let now = Utc::now();
        let next_month = if now.month() == 12 {
            now.with_month(1)
                .unwrap()
                .with_year(now.year() + 1)
                .unwrap()
        } else {
            now.with_month(now.month() + 1).unwrap()
        };
        let resets_at = next_month
            .with_day(1)
            .unwrap()
            .with_hour(0)
            .unwrap()
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap();

        let alert_level = if is_over_quota {
            Some("critical".to_string())
        } else if usage_pct >= 90.0 {
            Some("warning".to_string())
        } else if usage_pct >= 80.0 {
            Some("info".to_string())
        } else {
            None
        };

        // Compute trends
        let days_in_month = now.day() as f64;
        let daily_avg = if days_in_month > 0.0 {
            (app.current_month_messages_sent as f64 / days_in_month).round() as i64
        } else {
            0
        };
        let projected_monthly = daily_avg * 30;

        let quota_trend = if usage_pct > 80.0 {
            "critical"
        } else if usage_pct > 50.0 {
            "increasing"
        } else {
            "stable"
        };

        Self {
            id: app.application_id,
            name: app.name,
            desc: app.description,
            active: app.is_active,
            created: app.created_at,
            updated: app.updated_at,
            meta: app.app_meta.0,
            tier: app.tier,
            monthly_quota: app.monthly_message_quota,
            rate_limit: app.rate_limit_per_minute,
            retention_seconds: app.message_retention_seconds,
            keys: app.publishable_keys.0,
            webhooks: app.webhooks.0,
            quota_usage_pct: usage_pct,
            current_month: UsageStats {
                msg_sent: app.current_month_messages_sent,
                msg_received: app.current_month_messages_received,
                msg_proof: app.current_month_proofs_verified,
                msg_stored: app.current_month_bytes_stored,
                bytes_sent: app.current_month_bytes_sent,
                bytes_received: app.current_month_bytes_received,
                rate_hits: app.current_month_rate_limit_hits,
                cost: app.current_month_cost_cents,
            },
            lifetime: LifetimeStats {
                msg_sent: app.lifetime_messages_sent,
                cost: app.lifetime_cost_cents,
            },
            pricing_plan,
            quota_status: QuotaStatus {
                messages_used: app.current_month_messages_sent,
                messages_limit: app.monthly_message_quota,
                usage_pct,
                is_over_quota,
                overage_count,
                resets_at,
                alert_level,
            },
            trends: UsageTrends {
                daily_avg_messages: daily_avg,
                projected_monthly_messages: projected_monthly,
                quota_trend: quota_trend.to_string(),
            },
        }
    }
}
