use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Datelike, Utc};
use hyper::header;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vaultless_core::{
    PaginatedApplicationsWithKeys, get_global_mv_etag,
    models::{
        Application, ApplicationWithTier, CreateApplication, UpdateApplication,
        app_model::{chart::*, dto::*},
        usage::MetricCounters,
        user::User,
    },
    types::SubscriptionTier,
};

use chrono::Timelike;

// ============================================================================
// REQUEST/RESPONSE DTOs (UNMODIFIED)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct TrendsResponse {
    pub daily_average_messages: i64,
    pub weekly_average_messages: i64,
    pub growth_percentage_7d: f64,
    pub projected_monthly_cost_cents: i64,
    pub quota_trend: String,
}

#[derive(Debug, Serialize)]
pub struct CostBreakdownResponse {
    pub total_cost_cents: i64,
    pub breakdown: Vec<CostItem>,
}

#[derive(Debug, Serialize)]
pub struct CostItem {
    pub category: String,
    pub amount_cents: i64,
    pub unit: String,
    pub quantity: i64,
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
    pub application_id: Uuid,
    pub messages_used: i64,
    pub messages_limit: i64,
    pub usage_percentage: f64,
    pub is_over_quota: bool,
    pub overage_count: i64,
    pub resets_at: DateTime<Utc>,
    pub alert_level: Option<String>,
}

/// GET /analytics/quota/status
/// Real-time quota status check
/// GET /api/v1/applications/:id/quota-status
/// Real-time quota status for a specific application
pub async fn get_application_quota_status(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<QuotaStatusResponse>, ApiError> {
    // Get application with usage data
    let app = Application::find_owned_by_user(&*state.db, app_id, user.user_id).await?;

    // Calculate quota status
    let usage_percentage = app.quota_usage_percentage;
    let is_over_quota = app
        .monthly_message_quota
        .map(|quota| app.current_month_messages_sent > quota as i64)
        .unwrap_or(false);

    let overage_count = if is_over_quota {
        app.current_month_messages_sent - app.monthly_message_quota.unwrap_or(0) as i64
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
        messages_limit: app.monthly_message_quota.unwrap_or(0),
        usage_percentage,
        is_over_quota,
        overage_count,
        resets_at,
        alert_level,
    }))
}

/// GET /api/v1/applications/:id/costs
/// Detailed cost breakdown
pub async fn get_application_cost_breakdown(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(user): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<CostBreakdownResponse>, ApiError> {
    let app = Application::find_owned_by_user(&*state.db, app_id, user.user_id).await?;

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

/// GET /api/v1/applications/:id/export?format=csv
/// Export application usage data
pub async fn export_application_usage(
    Path(app_id): Path<Uuid>,
    Query(query): Query<ExportQuery>,
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let app = Application::find_owned_by_user(&*state.db, app_id, session.user_id).await?;

    match query.format {
        ExportFormat::Json => Ok(Json(app).into_response()),
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

/// GET /api/v1/applications/:id/trends
/// Calculate usage trends and growth rates
pub async fn get_application_trends(
    Path(app_id): Path<Uuid>,
    SessionDataUserExt(session): SessionDataUserExt,
    State(state): State<AppState>,
) -> Result<Json<TrendsResponse>, ApiError> {
    let app = Application::find_owned_by_user(&*state.db, app_id, session.user_id).await?;

    // Calculate trends
    let daily_average = app.last_30d_messages_sent as f64 / 30.0;
    let weekly_average = app.last_7d_messages_sent as f64 / 7.0;

    // Simple growth calculation (7d vs previous 7d approximation)
    // For more accurate, you'd query the previous period from the DB
    let growth_7d = if app.last_7d_messages_sent > 0 {
        ((weekly_average - daily_average) / daily_average * 100.0).round()
    } else {
        0.0
    };

    let cost_trend = if app.last_30d_cost_cents > 0 {
        let daily_cost = app.last_30d_cost_cents as f64 / 30.0;
        let projected_monthly = daily_cost * 30.0;
        projected_monthly.round() as i64
    } else {
        0
    };

    Ok(Json(TrendsResponse {
        daily_average_messages: daily_average.round() as i64,
        weekly_average_messages: weekly_average.round() as i64,
        growth_percentage_7d: growth_7d,
        projected_monthly_cost_cents: cost_trend,
        quota_trend: if app.quota_usage_percentage > 0.0 {
            "increasing".to_string()
        } else {
            "stable".to_string()
        },
    }))
}

// ============================================================================
// HELPER FUNCTIONS (UNMODIFIED)
// ============================================================================

fn generate_usage_csv(app: &ApplicationWithUsageResponse) -> Result<String, ApiError> {
    let mut csv = String::from("metric,current_month,last_7d,last_30d,lifetime\n");

    csv.push_str(&format!(
        "messages_sent,{},{},{},{}\n",
        app.current_month_messages_sent,
        app.last_7d_messages_sent,
        app.last_30d_messages_sent,
        app.lifetime_messages_sent
    ));

    csv.push_str(&format!(
        "messages_received,{},{},{},{}\n",
        app.current_month_messages_received,
        0, // Not tracked in 7d
        0, // Not tracked in 30d
        app.lifetime_messages_received
    ));

    csv.push_str(&format!(
        "bytes_sent,{},{},{},{}\n",
        app.current_month_bytes_sent,
        app.last_7d_bytes_sent,
        app.last_30d_bytes_sent,
        app.lifetime_bytes_sent
    ));

    csv.push_str(&format!(
        "bytes_received,{},{},{},{}\n",
        app.current_month_bytes_received,
        app.last_7d_bytes_received,
        app.last_30d_bytes_received,
        app.lifetime_bytes_received
    ));

    csv.push_str(&format!(
        "cost_cents,{},{},{},{}\n",
        app.current_month_cost_cents,
        app.last_7d_cost_cents,
        app.last_30d_cost_cents,
        app.lifetime_cost_cents
    ));

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
