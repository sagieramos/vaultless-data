//! Analytics handlers for applications.
//!
//! Provides endpoints for:
//! - Data export (JSON/CSV)
//! - Monthly revenue analytics

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use hyper::header;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use vaultless_core::{
    models::applications::dto::Application,
    models::usage::application::monthly_revenue::{
        MonthlyRevenueData, PaginatedMonthlyApplicationRevenue, RevenueChartData,
    },
    types::SubscriptionTier,
};

// ============================================================================
// REQUEST/RESPONSE DTOs
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeOption {
    /// Target subscription tier
    pub tier: String,
    /// Monthly price in cents (None for custom/enterprise pricing)
    pub monthly_price_cents: Option<i32>,
    /// List of benefits for this tier
    pub benefits: Vec<String>,
    /// URL to upgrade to this tier
    pub upgrade_url: String,
}

#[derive(Debug, Deserialize, Clone, Copy, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    pub format: ExportFormat,
    /// Whether to include pricing plan information in the export
    #[serde(default = "default_include_pricing")]
    pub include_pricing_plan: bool,
}

fn default_include_pricing() -> bool {
    false // Default to false to maintain backward compatibility
}

/// Export application usage data in JSON or CSV format
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/export",
    params(
        ("application_id" = Uuid, Path, description = "Application ID"),
        ("format" = ExportFormat, Query, description = "Export format: json or csv"),
        ("include_pricing_plan" = Option<bool>, Query, description = "Include pricing plan information in the export (default: false)")
    ),
    responses(
        (status = 200, description = "Usage data exported successfully (JSON or CSV based on format parameter)"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn export_application_usage(
    Path(app_id): Path<Uuid>,
    Query(query): Query<ExportQuery>,
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    // Always fetch with pricing plan for complete export
    let result = Application::find_owned_by_user(
        state.db.as_ref(),
        app_id,
        session.user_id,
        true,
    )
    .await?;

    let dashboard = super::application::dto::ApplicationDashboardResponse::from((
        result.application,
        result.pricing_plan,
    ));
    let app_name = dashboard.name.replace(' ', "_");

    match query.format {
        ExportFormat::Json => Ok(Json(dashboard).into_response()),

        ExportFormat::Csv => {
            let csv_data = generate_usage_csv(&dashboard, query.include_pricing_plan);

            let headers = [
                (
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/csv; charset=utf-8"),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    header::HeaderValue::from_str(&format!(
                        "attachment; filename=\"{}_analytics_export.csv\"",
                        app_name
                    ))
                    .unwrap(),
                ),
            ];

            Ok((headers, csv_data).into_response())
        }
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Generates a comprehensive CSV export of application analytics data.
/// Format follows enterprise reporting standards with clear section headers.
fn generate_usage_csv(
    dashboard: &super::application::dto::ApplicationDashboardResponse,
    include_pricing: bool,
) -> String {
    let mut csv = String::with_capacity(4096);

    // CSV Header with metadata
    csv.push_str("# Application Analytics Export\n");
    csv.push_str(&format!("# Generated: {}\n", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
    csv.push_str(&format!("# Application ID: {}\n", dashboard.id));
    csv.push_str("#\n");

    // Section 1: Application Information
    csv.push_str("section,field,value\n");
    csv.push_str(&format!("application,id,{}\n", dashboard.id));
    csv.push_str(&format!("application,name,\"{}\"\n", escape_csv(&dashboard.name)));
    csv.push_str(&format!(
        "application,description,\"{}\"\n",
        dashboard.desc.as_deref().map(escape_csv).unwrap_or_default()
    ));
    csv.push_str(&format!("application,active,{}\n", dashboard.active));
    csv.push_str(&format!("application,created_at,{}\n", dashboard.created.format("%Y-%m-%dT%H:%M:%SZ")));
    csv.push_str(&format!("application,updated_at,{}\n", dashboard.updated.format("%Y-%m-%dT%H:%M:%SZ")));

    // Section 2: Subscription & Limits
    csv.push_str(&format!(
        "subscription,tier,{}\n",
        dashboard.tier.as_deref().unwrap_or("none")
    ));
    csv.push_str(&format!(
        "subscription,monthly_message_quota,{}\n",
        dashboard.monthly_quota.map(|q| q.to_string()).unwrap_or_else(|| "unlimited".to_string())
    ));
    csv.push_str(&format!(
        "subscription,rate_limit_per_minute,{}\n",
        dashboard.rate_limit.map(|r| r.to_string()).unwrap_or_else(|| "n/a".to_string())
    ));
    csv.push_str(&format!(
        "subscription,retention_seconds,{}\n",
        dashboard.retention_seconds.map(|r| r.to_string()).unwrap_or_else(|| "n/a".to_string())
    ));

    // Section 3: Current Month Usage
    csv.push_str(&format!("current_month,messages_sent,{}\n", dashboard.current_month.msg_sent));
    csv.push_str(&format!("current_month,messages_received,{}\n", dashboard.current_month.msg_received));
    csv.push_str(&format!("current_month,proofs_verified,{}\n", dashboard.current_month.msg_proof));
    csv.push_str(&format!("current_month,bytes_stored,{}\n", dashboard.current_month.msg_stored));
    csv.push_str(&format!("current_month,bytes_sent,{}\n", dashboard.current_month.bytes_sent));
    csv.push_str(&format!("current_month,bytes_received,{}\n", dashboard.current_month.bytes_received));
    csv.push_str(&format!("current_month,rate_limit_hits,{}\n", dashboard.current_month.rate_hits));

    // Section 4: Lifetime Usage
    csv.push_str(&format!("lifetime,messages_sent,{}\n", dashboard.lifetime.msg_sent));

    // Section 5: Quota Status
    csv.push_str(&format!("quota,messages_used,{}\n", dashboard.quota_status.messages_used));
    csv.push_str(&format!(
        "quota,messages_limit,{}\n",
        dashboard.quota_status.messages_limit.map(|l| l.to_string()).unwrap_or_else(|| "unlimited".to_string())
    ));
    csv.push_str(&format!("quota,usage_percentage,{:.2}\n", dashboard.quota_status.usage_pct));
    csv.push_str(&format!("quota,is_over_quota,{}\n", dashboard.quota_status.is_over_quota));
    csv.push_str(&format!("quota,overage_count,{}\n", dashboard.quota_status.overage_count));
    csv.push_str(&format!("quota,resets_at,{}\n", dashboard.quota_status.resets_at.format("%Y-%m-%dT%H:%M:%SZ")));
    csv.push_str(&format!(
        "quota,alert_level,{}\n",
        dashboard.quota_status.alert_level.as_deref().unwrap_or("none")
    ));

    // Section 6: Trends
    csv.push_str(&format!("trends,daily_average_messages,{}\n", dashboard.trends.daily_avg_messages));
    csv.push_str(&format!("trends,projected_monthly_messages,{}\n", dashboard.trends.projected_monthly_messages));
    csv.push_str(&format!("trends,quota_trend,{}\n", dashboard.trends.quota_trend));

    // Section 7: Keys Summary
    csv.push_str(&format!("keys,total_count,{}\n", dashboard.keys.len()));
    csv.push_str(&format!(
        "keys,active_count,{}\n",
        dashboard.keys.iter().filter(|k| k.is_active).count()
    ));

    // Section 8: Webhooks Summary
    csv.push_str(&format!("webhooks,total_count,{}\n", dashboard.webhooks.len()));
    csv.push_str(&format!(
        "webhooks,active_count,{}\n",
        dashboard.webhooks.iter().filter(|w| w.is_active).count()
    ));

    // Section 9: Pricing Plan (if included and available)
    if include_pricing {
        if let Some(ref plan) = dashboard.pricing_plan {
            csv.push_str(&format!("pricing_plan,id,{}\n", plan.id));
            csv.push_str(&format!("pricing_plan,name,\"{}\"\n", escape_csv(&plan.name)));
            csv.push_str(&format!("pricing_plan,pricing_mode,{:?}\n", plan.pricing_mode));
            csv.push_str(&format!(
                "pricing_plan,price_per_message_cents,{}\n",
                plan.price_per_message_cents.map(|p| p.to_string()).unwrap_or_else(|| "n/a".to_string())
            ));
            csv.push_str(&format!(
                "pricing_plan,price_per_gb_cents,{}\n",
                plan.price_per_gb_cents.map(|p| p.to_string()).unwrap_or_else(|| "n/a".to_string())
            ));
            csv.push_str(&format!(
                "pricing_plan,price_per_proof_cents,{}\n",
                plan.price_per_proof_cents.map(|p| p.to_string()).unwrap_or_else(|| "n/a".to_string())
            ));
            csv.push_str(&format!(
                "pricing_plan,prepaid_amount_cents,{}\n",
                plan.prepaid_amount_cents.map(|p| p.to_string()).unwrap_or_else(|| "n/a".to_string())
            ));
            csv.push_str(&format!("pricing_plan,is_default,{}\n", plan.is_default));
            csv.push_str(&format!("pricing_plan,attached_at,{}\n", plan.attached_at.format("%Y-%m-%dT%H:%M:%SZ")));
        } else {
            csv.push_str("pricing_plan,status,not_attached\n");
        }
    }

    csv
}

/// Escapes special characters for CSV format (RFC 4180 compliant)
fn escape_csv(s: &str) -> String {
    if s.contains('"') || s.contains(',') || s.contains('\n') || s.contains('\r') {
        format!("{}", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ============================================================================
// UPGRADE RECOMMENDATIONS
// ============================================================================

/// Response containing upgrade recommendations for an application
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeRecommendationsResponse {
    /// Current subscription tier
    pub current_tier: String,
    /// Available upgrade options
    pub recommendations: Vec<UpgradeOption>,
    /// Whether the user is on the highest tier
    pub is_max_tier: bool,
}

/// Get upgrade recommendations for a specific application based on its current tier
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/upgrade-recommendations",
    params(
        ("application_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Upgrade recommendations retrieved successfully", body = UpgradeRecommendationsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_application_upgrade_recommendations(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<UpgradeRecommendationsResponse>, ApiError> {
    let result =
        Application::find_owned_by_user(state.db.as_ref(), app_id, user.user_id, false).await?;

    let current_tier = result
        .application
        .tier
        .as_deref()
        .and_then(|t| t.parse::<SubscriptionTier>().ok())
        .unwrap_or(SubscriptionTier::Free);

    let recommendations = get_upgrade_options(&current_tier);
    let is_max_tier = matches!(current_tier, SubscriptionTier::Enterprise);

    Ok(Json(UpgradeRecommendationsResponse {
        current_tier: format!("{:?}", current_tier),
        recommendations,
        is_max_tier,
    }))
}

/// Get upgrade options based on current tier
fn get_upgrade_options(current_tier: &SubscriptionTier) -> Vec<UpgradeOption> {
    const BASE_URL: &str = "https://vaultless.dev/pricing";

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
                upgrade_url: format!("{BASE_URL}?plan=starter"),
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
                upgrade_url: format!("{BASE_URL}?plan=pro"),
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
            upgrade_url: format!("{BASE_URL}?plan=pro"),
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
            upgrade_url: format!("{BASE_URL}?plan=enterprise&contact=sales"),
        }],
        SubscriptionTier::Enterprise => vec![],
    }
}

// ============================================================================
// MONTHLY REVENUE ENDPOINTS
// ============================================================================

#[derive(Debug, Deserialize, ToSchema, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyRevenueQuery {
    #[serde(default = "default_months_back")]
    pub months_back: i32,
}

fn default_months_back() -> i32 {
    12
}

#[derive(Debug, Deserialize, ToSchema, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyBreakdownQuery {
    pub month: Option<DateTime<Utc>>,
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    20
}

/// Get monthly revenue data for a specific application
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/monthly-revenue",
    params(
        ("application_id" = Uuid, Path, description = "Application ID"),
        MonthlyRevenueQuery
    ),
    responses(
        (status = 200, description = "Monthly revenue data retrieved successfully", body = RevenueChartData),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_application_monthly_revenue(
    Path(app_id): Path<Uuid>,
    Query(query): Query<MonthlyRevenueQuery>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<RevenueChartData>, ApiError> {
    let chart_data = MonthlyRevenueData::get_chart_data_for_developer_application(
        state.db.as_ref(),
        user.user_id,
        app_id,
        query.months_back,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(chart_data))
}

/// Get monthly revenue data for all applications belonging to the developer
#[utoipa::path(
    get,
    path = "/dev/analytics/monthly-revenue",
    params(
        MonthlyRevenueQuery
    ),
    responses(
        (status = 200, description = "Monthly revenue data retrieved successfully", body = RevenueChartData),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_developer_monthly_revenue(
    Query(query): Query<MonthlyRevenueQuery>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<RevenueChartData>, ApiError> {
    let chart_data = MonthlyRevenueData::get_chart_data_for_developer(
        state.db.as_ref(),
        user.user_id,
        query.months_back,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(chart_data))
}

/// Get monthly revenue breakdown by application for a specific month
#[utoipa::path(
    get,
    path = "/dev/analytics/monthly-breakdown",
    params(
        MonthlyBreakdownQuery
    ),
    responses(
        (status = 200, description = "Monthly breakdown retrieved successfully", body = PaginatedMonthlyApplicationRevenue),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_monthly_revenue_breakdown(
    Query(query): Query<MonthlyBreakdownQuery>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<PaginatedMonthlyApplicationRevenue>, ApiError> {
    let month = query.month.unwrap_or_else(|| Utc::now());

    let breakdown = MonthlyRevenueData::get_monthly_totals_by_application(
        state.db.as_ref(),
        user.user_id,
        month,
        query.page,
        query.page_size,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(breakdown))
}
