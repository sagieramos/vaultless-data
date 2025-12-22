//! Request and Response DTOs for application handlers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use vaultless_core::models::{
    Application,
    app_model::{dto::*, integrity::dto::IntegrityConfig},
    usage::MetricCounters,
};

// =============================================================================
// Application DTOs
// =============================================================================

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
    pub meta: vaultless_core::models::app_model::integrity::dto::AppMetaData,

    // Subscription tier info
    /// Subscription tier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Monthly message quota
    pub monthly_quota: i64,
    /// Rate limit per minute
    pub rate_limit: i32,
    /// Message retention in seconds
    pub retention_seconds: i64,

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

impl From<ApplicationWithUsage> for ApplicationDashboardResponse {
    fn from(app: ApplicationWithUsage) -> Self {
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
            quota_usage_pct: app
                .quota_usage_percentage
                .to_string()
                .parse::<f64>()
                .unwrap_or(0.0),
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
        }
    }
}
