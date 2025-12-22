//! Analytics handlers for applications.
//!
//! Provides endpoints for:
//! - Quota status monitoring
//! - Cost breakdown analysis
//! - Usage trends
//! - Data export (JSON/CSV)

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Datelike, Timelike, Utc};
use hyper::header;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use vaultless_core::{models::app_model::dto::*, types::SubscriptionTier};

// ============================================================================
// REQUEST/RESPONSE DTOs
// ============================================================================

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrendsResponse {
    pub daily_average_messages: i64,
    pub projected_monthly_cost_cents: i64,
    pub quota_trend: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CostBreakdownResponse {
    pub total_cost_cents: i64,
    pub breakdown: Vec<CostItem>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CostItem {
    pub category: String,
    pub amount_cents: i64,
    pub unit: String,
    pub quantity: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeOption {
    pub tier: String,
    pub monthly_price_cents: Option<i32>,
    pub benefits: Vec<String>,
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
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsResponse<T: ToSchema> {
    pub success: bool,
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade_message: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuotaStatusResponse {
    pub application_id: Uuid,
    pub messages_used: i64,
    pub messages_limit: i64,
    pub usage_percentage: f64,
    pub is_over_quota: bool,
    pub overage_count: i64,
    pub resets_at: DateTime<Utc>,
    pub alert_level: Option<String>,
}

/// Get real-time quota status for a specific application
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/quota-status",
    params(
        ("application_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Quota status retrieved successfully", body = QuotaStatusResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_application_quota_status(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<QuotaStatusResponse>, ApiError> {
    let app = Application::find_owned_by_user(state.db.as_ref(), app_id, user.user_id).await?;

    // Calculate quota status
    let usage_percentage = app
        .quota_usage_percentage
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0);

    let is_over_quota = app.current_month_messages_sent > app.monthly_message_quota;

    let overage_count = if is_over_quota {
        app.current_month_messages_sent - app.monthly_message_quota
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
    } else if usage_percentage >= 90.0 {
        Some("warning".to_string())
    } else if usage_percentage >= 80.0 {
        Some("info".to_string())
    } else {
        None
    };

    Ok(Json(QuotaStatusResponse {
        application_id: app.application_id,
        messages_used: app.current_month_messages_sent,
        messages_limit: app.monthly_message_quota,
        usage_percentage,
        is_over_quota,
        overage_count,
        resets_at,
        alert_level,
    }))
}

/// Get detailed cost breakdown for an application
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/costs",
    params(
        ("application_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Cost breakdown retrieved successfully", body = CostBreakdownResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_application_cost_breakdown(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<CostBreakdownResponse>, ApiError> {
    let app = Application::find_owned_by_user(state.db.as_ref(), app_id, user.user_id).await?;

    // Calculate cost breakdown (based on your pricing model)
    let message_cost = (app.current_month_messages_sent as f64 / 1000.0) * 1.0; // $0.01 per 1000
    let bandwidth_cost = ((app.current_month_bytes_sent + app.current_month_bytes_received) as f64
        / 1_000_000_000.0)
        * 10.0; // $0.10 per GB
    let storage_cost = (app.current_month_bytes_stored as f64 / 1_000_000_000.0) * 5.0; // $0.05 per GB/month
    let proof_cost = (app.current_month_proofs_verified as f64 / 1000.0) * 0.5; // $0.005 per 1000

    Ok(Json(CostBreakdownResponse {
        total_cost_cents: app.current_month_cost_cents,
        breakdown: vec![
            CostItem {
                category: "Messages".to_string(),
                amount_cents: (message_cost * 100.0).round() as i64,
                unit: "per 1000 messages".to_string(),
                quantity: app.current_month_messages_sent,
            },
            CostItem {
                category: "Bandwidth".to_string(),
                amount_cents: (bandwidth_cost * 100.0).round() as i64,
                unit: "per GB".to_string(),
                quantity: (app.current_month_bytes_sent + app.current_month_bytes_received)
                    / 1_000_000_000,
            },
            CostItem {
                category: "Storage".to_string(),
                amount_cents: (storage_cost * 100.0).round() as i64,
                unit: "per GB/month".to_string(),
                quantity: app.current_month_bytes_stored / 1_000_000_000,
            },
            CostItem {
                category: "Proofs".to_string(),
                amount_cents: (proof_cost * 100.0).round() as i64,
                unit: "per 1000 proofs".to_string(),
                quantity: app.current_month_proofs_verified,
            },
        ],
    }))
}

/// Export application usage data in JSON or CSV format
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/export",
    params(
        ("application_id" = Uuid, Path, description = "Application ID"),
        ("format" = ExportFormat, Query, description = "Export format: json or csv")
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
    let app = Application::find_owned_by_user(state.db.as_ref(), app_id, session.user_id).await?;

    match query.format {
        ExportFormat::Json => Ok(Json(&app).into_response()),
        ExportFormat::Csv => {
            let csv_data = generate_usage_csv(&app)?;

            let headers = [
                (
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("text/csv"),
                ),
                (
                    header::CONTENT_DISPOSITION,
                    header::HeaderValue::from_str(&format!(
                        "attachment; filename=\"{}_usage.csv\"",
                        app.name.replace(" ", "_")
                    ))
                    .unwrap(),
                ),
            ];

            Ok((headers, csv_data).into_response())
        }
    }
}

/// Get usage trends for an application
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/trends",
    params(
        ("application_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Trends retrieved successfully", body = TrendsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_application_trends(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<TrendsResponse>, ApiError> {
    let app = Application::find_owned_by_user(state.db.as_ref(), app_id, session.user_id).await?;

    // Calculate trends based on current month data
    // Note: For more accurate trends, you'd query from usage_metrics_daily
    let now = Utc::now();
    let days_in_month = now.day() as f64;
    let daily_average = if days_in_month > 0.0 {
        (app.current_month_messages_sent as f64 / days_in_month).round() as i64
    } else {
        0
    };

    let projected_monthly = daily_average * 30;

    let usage_percentage = app
        .quota_usage_percentage
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0);

    let quota_trend = if usage_percentage > 80.0 {
        "critical"
    } else if usage_percentage > 50.0 {
        "increasing"
    } else {
        "stable"
    };

    Ok(Json(TrendsResponse {
        daily_average_messages: daily_average,
        projected_monthly_cost_cents: projected_monthly,
        quota_trend: quota_trend.to_string(),
    }))
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn generate_usage_csv(app: &ApplicationWithUsage) -> Result<String, ApiError> {
    let mut csv = String::from("metric,current_month,lifetime\n");

    csv.push_str(&format!(
        "messages_sent,{},{}\n",
        app.current_month_messages_sent, app.lifetime_messages_sent
    ));

    csv.push_str(&format!(
        "messages_received,{},n/a\n",
        app.current_month_messages_received
    ));

    csv.push_str(&format!(
        "bytes_sent,{},n/a\n",
        app.current_month_bytes_sent
    ));

    csv.push_str(&format!(
        "bytes_received,{},n/a\n",
        app.current_month_bytes_received
    ));

    csv.push_str(&format!(
        "cost_cents,{},{}\n",
        app.current_month_cost_cents, app.lifetime_cost_cents
    ));

    Ok(csv)
}

/// Get upgrade recommendations based on current tier
#[allow(dead_code)]
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
