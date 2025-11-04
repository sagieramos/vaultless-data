// vaultless-api/src/handlers/analytics.rs
use axum::{
    Json,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::middleware::api_key_auth::AuthenticatedApiKey;

use crate::middleware::error::ApiError;
use crate::services::analytics::{AnalyticsDashboard, AnalyticsService, TimeSeriesDataPoint};
use crate::state::AppState;
use vaultless_core::SubscriptionTier;

// ============================================================================
// REQUEST/RESPONSE DTOs (UNMODIFIED)
// ============================================================================

// ... (All structs like TimeSeriesQuery, ExportFormat, AnalyticsResponse, QuotaStatusResponse) ...

#[derive(Debug, Deserialize)]
pub struct TimeSeriesQuery {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    #[serde(default = "default_interval")]
    pub interval: String, // "day", "week", "hour"
}

fn default_interval() -> String {
    "day".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeOption {
    pub tier: String,
    pub monthly_price_cents: Option<i32>,
    pub benefits: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    pub format: ExportFormat,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AnalyticsResponse<T> {
    pub success: bool,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QuotaStatusResponse {
    pub messages_used: i64,
    pub messages_limit: i64,
    pub usage_percentage: f64,
    pub is_over_quota: bool,
    pub overage_count: i64,
    pub resets_at: DateTime<Utc>,
    pub alert_level: Option<String>,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// GET /analytics/dashboard
/// Main analytics dashboard with overview, trends, costs
#[axum::debug_handler]
pub async fn get_dashboard(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    // Check tier access
    if api_key.tier == SubscriptionTier::Free {
        return Err(ApiError::forbidden(
            "Analytics dashboard requires Starter tier or higher. Upgrade at https://vaultless.com/pricing",
        ));
    }

    let analytics_service = AnalyticsService::new(state.db.clone());

    let dashboard = analytics_service
        .get_dashboard(api_key.id, api_key.tier)
        .await?;

    let upgrade_msg = if api_key.tier == SubscriptionTier::Starter {
        Some("Upgrade to Pro for 90-day analytics and real-time webhooks".to_string())
    } else {
        None
    };

    Ok(Json(AnalyticsResponse {
        success: true,
        data: dashboard,
        upgrade_message: upgrade_msg,
    }))
}

/// GET /analytics/usage/timeseries
/// Time-series data for charts (tier-limited historical access)
pub async fn get_usage_timeseries(
    State(state): State<AppState>,
    Query(query): Query<TimeSeriesQuery>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    // Tier check
    if api_key.tier == SubscriptionTier::Free {
        return Err(ApiError::forbidden(
            "Time-series analytics requires Starter tier or higher",
        ));
    }

    let analytics_service = AnalyticsService::new(state.db.clone());

    // Default to last 7 days if not specified
    let end = query.end.unwrap_or_else(Utc::now);

    // Default to last 7 days from the end point, respecting the tier limit
    let default_duration = match api_key.tier {
        SubscriptionTier::Starter => chrono::Duration::days(7),
        SubscriptionTier::Pro => chrono::Duration::days(90),
        SubscriptionTier::Enterprise => chrono::Duration::days(365),
        SubscriptionTier::Free => chrono::Duration::days(0), // Should be caught by the check above
    };

    let start = query.start.unwrap_or_else(|| end - default_duration);

    let timeseries = analytics_service
        .get_time_series(api_key.id, api_key.tier, start, end)
        .await?;

    let upgrade_msg = match api_key.tier {
        SubscriptionTier::Starter => {
            Some("Viewing last 7 days. Upgrade to Pro for 90-day history".to_string())
        }
        SubscriptionTier::Pro => {
            Some("Viewing last 90 days. Upgrade to Enterprise for unlimited history".to_string())
        }
        _ => None,
    };

    Ok(Json(AnalyticsResponse {
        success: true,
        data: timeseries,
        upgrade_message: upgrade_msg,
    }))
}

/// GET /analytics/quota/status
/// Real-time quota status check
pub async fn get_quota_status(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    let analytics_service = AnalyticsService::new(state.db.clone());
    let quota_status = analytics_service.get_quota_status(api_key.id).await?;

    // Determine alert level based on QuotaStatus data
    let alert_level = if quota_status.is_over_quota {
        Some("critical".to_string())
    } else if quota_status.usage_percentage >= 90.0 {
        Some("warning".to_string())
    } else if quota_status.usage_percentage >= 80.0 {
        Some("info".to_string()) // Or maybe "low_risk"
    } else {
        None
    };

    Ok(Json(QuotaStatusResponse {
        messages_used: quota_status.messages_used,
        messages_limit: quota_status.messages_limit,
        usage_percentage: quota_status.usage_percentage,
        is_over_quota: quota_status.is_over_quota,
        overage_count: quota_status.overage_count,
        resets_at: quota_status.resets_at,
        alert_level,
    }))
}

/// GET /analytics/costs
/// Detailed cost breakdown by operation type
pub async fn get_cost_breakdown(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    // Only Pro+ can see cost breakdowns
    if matches!(
        api_key.tier,
        SubscriptionTier::Free | SubscriptionTier::Starter
    ) {
        return Err(ApiError::forbidden(
            "Cost analytics requires Pro tier or higher",
        ));
    }

    let analytics_service = AnalyticsService::new(state.db.clone());
    // FIX: Call get_dashboard and extract the cost_breakdown field
    let dashboard = analytics_service
        .get_dashboard(api_key.id, api_key.tier)
        .await?;

    Ok(Json(AnalyticsResponse {
        success: true,
        data: dashboard.cost_breakdown,
        upgrade_message: None,
    }))
}

/// POST /analytics/export
/// Export usage data as CSV or JSON (Pro+ only)pub async fn export_analytics
#[axum::debug_handler]
pub async fn export_analytics(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
    Json(query): Json<ExportQuery>,
) -> Result<Response, ApiError> {
    // Return the concrete axum::response::Response
    // Pro+ only feature
    if !matches!(
        api_key.tier,
        SubscriptionTier::Pro | SubscriptionTier::Enterprise
    ) {
        return Err(ApiError::forbidden(
            "Data export requires Pro tier or higher. Upgrade at https://vaultless.com/pricing",
        ));
    }

    let analytics_service = AnalyticsService::new(state.db.clone());

    // Determine the maximum range allowed by the tier
    let max_days = match api_key.tier {
        SubscriptionTier::Pro => 90,
        SubscriptionTier::Enterprise => 365,
        _ => 0, // Should be caught by the check above
    };

    let end = query.end.unwrap_or_else(Utc::now);
    let start_default = end - chrono::Duration::days(max_days);

    // Use the query start if provided, otherwise default to the maximum allowed range
    let start = query.start.unwrap_or(start_default);

    // Validate if the requested range exceeds the tier limit (redundant due to service check, but good defense)
    let requested_duration = end.signed_duration_since(start).num_days();
    if requested_duration > max_days {
        return Err(ApiError::bad_request(format!(
            "Your tier allows a maximum export of {} days of historical data.",
            max_days
        )));
    }

    let timeseries = analytics_service
        .get_time_series(api_key.id, api_key.tier, start, end)
        .await?;

    let response = match query.format {
        ExportFormat::Json => Json(AnalyticsResponse {
            success: true,
            data: timeseries,
            upgrade_message: None,
        })
        .into_response(),
        ExportFormat::Csv => {
            let csv_data = generate_csv(&timeseries)?;

            use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderValue};

            let headers = [
                (CONTENT_TYPE, HeaderValue::from_static("text/csv")),
                (
                    CONTENT_DISPOSITION,
                    HeaderValue::from_static("attachment; filename=\"analytics.csv\""),
                ),
            ];

            (headers, csv_data).into_response()
        }
    };

    Ok(response)
}

/// GET /analytics/trends
/// Week-over-week growth trends
pub async fn get_usage_trends(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    // Starter+ feature
    if api_key.tier == SubscriptionTier::Free {
        return Err(ApiError::forbidden(
            "Usage trends require Starter tier or higher",
        ));
    }

    let analytics_service = AnalyticsService::new(state.db.clone());
    // FIX: Call get_dashboard and extract the trends field
    let dashboard = analytics_service
        .get_dashboard(api_key.id, api_key.tier)
        .await?;

    Ok(Json(AnalyticsResponse {
        success: true,
        data: dashboard.trends,
        upgrade_message: None,
    }))
}

/// GET /analytics/overview
/// High-level usage summary
pub async fn get_usage_overview(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    if api_key.tier == SubscriptionTier::Free {
        return Err(ApiError::forbidden(
            "Usage overview requires Starter tier or higher",
        ));
    }

    let analytics_service = AnalyticsService::new(state.db.clone());

    // FIX: Call get_dashboard and extract the overview field
    let dashboard = analytics_service
        .get_dashboard(api_key.id, api_key.tier)
        .await?;

    Ok(Json(AnalyticsResponse {
        success: true,
        data: dashboard.overview,
        upgrade_message: None,
    }))
}

/// GET /analytics/tier
/// Current tier information and features
pub async fn get_tier_info(
    State(state): State<AppState>,
    AuthenticatedApiKey(api_key): AuthenticatedApiKey,
) -> Result<impl IntoResponse, ApiError> {
    let analytics_service = AnalyticsService::new(state.db.clone());

    // FIX: Call get_dashboard and extract the tier_info field
    let dashboard = analytics_service
        .get_dashboard(api_key.id, api_key.tier)
        .await?;

    let upgrade_options = get_upgrade_recommendations(&api_key.tier);

    #[derive(Serialize)]
    struct TierInfoResponse {
        current_tier: vaultless_core::SubscriptionTier, // Use vaultless_core::SubscriptionTier
        features: Vec<String>,
        limits: TierLimits,
        upgrade_options: Vec<UpgradeOption>,
    }

    #[derive(Serialize)]
    struct TierLimits {
        monthly_quota: i32,
        rate_limit_per_minute: i32,
        retention_days: i32,
        analytics_history_days: i32,
    }

    let analytics_days = match api_key.tier {
        SubscriptionTier::Free => 0,
        SubscriptionTier::Starter => 7,
        SubscriptionTier::Pro => 90,
        SubscriptionTier::Enterprise => 365,
    };

    Ok(Json(AnalyticsResponse {
        success: true,
        data: TierInfoResponse {
            current_tier: dashboard.tier_info.current_tier,
            features: dashboard.tier_info.features,
            limits: TierLimits {
                monthly_quota: dashboard.tier_info.monthly_quota,
                rate_limit_per_minute: dashboard.tier_info.rate_limit_per_minute,
                retention_days: dashboard.tier_info.retention_days,
                analytics_history_days: analytics_days,
            },
            upgrade_options,
        },
        upgrade_message: None,
    }))
}

// ============================================================================
// HELPER FUNCTIONS (UNMODIFIED)
// ============================================================================

/// Generate CSV from time-series data
fn generate_csv(data: &[TimeSeriesDataPoint]) -> Result<String, ApiError> {
    let mut csv =
        String::from("timestamp,messages_sent,messages_received,proofs_verified,bytes_stored\n");

    for point in data {
        csv.push_str(&format!(
            "{},{},{},{},{}\n",
            point.timestamp.to_rfc3339(),
            point.messages_sent,
            point.messages_received,
            point.proofs_verified,
            point.bytes_stored
        ));
    }

    Ok(csv)
}

/// Get upgrade recommendations based on current tier
fn get_upgrade_recommendations(current_tier: &SubscriptionTier) -> Vec<UpgradeOption> {
    match current_tier {
        SubscriptionTier::Free => vec![
            UpgradeOption {
                tier: "Starter".to_string(),
                monthly_price_cents: Some(2900),
                benefits: vec![
                    "50,000 messages/month".to_string(),
                    "7-day analytics".to_string(),
                    "300 req/min rate limit".to_string(),
                    "Email support".to_string(),
                ],
            },
            UpgradeOption {
                tier: "Pro".to_string(),
                monthly_price_cents: Some(14900),
                benefits: vec![
                    "500,000 messages/month".to_string(),
                    "90-day analytics".to_string(),
                    "Real-time webhooks".to_string(),
                    "Priority support".to_string(),
                ],
            },
        ],
        SubscriptionTier::Starter => vec![UpgradeOption {
            tier: "Pro".to_string(),
            monthly_price_cents: Some(14900),
            benefits: vec![
                "10x more messages".to_string(),
                "90-day analytics (vs 7 days)".to_string(),
                "Real-time webhooks".to_string(),
                "Priority support".to_string(),
            ],
        }],
        SubscriptionTier::Pro => vec![UpgradeOption {
            tier: "Enterprise".to_string(),
            monthly_price_cents: None,
            benefits: vec![
                "Unlimited messages".to_string(),
                "Full analytics history".to_string(),
                "Custom SLA guarantees".to_string(),
                "Dedicated support".to_string(),
            ],
        }],
        SubscriptionTier::Enterprise => vec![],
    }
}
