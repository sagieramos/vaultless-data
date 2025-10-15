use axum::{Extension, Json, extract::State};
use serde::Serialize;
use vaultless_core::{ApiKey, DailyUsageSummary, MonthlyTotal, WeeklyUsageSummary};

use crate::{middleware::error::ApiError, state::AppState};

#[derive(Debug, Serialize)]
pub struct AnalyticsDashboard {
    pub current_month: MonthlyTotal,
    pub last_7_days: Vec<DailyUsageSummary>,
    pub last_4_weeks: Vec<WeeklyUsageSummary>,
    pub quota_usage: QuotaUsage,
    pub trends: vaultless_core::UsageTrends,
}

#[derive(Debug, Serialize)]
pub struct QuotaUsage {
    pub monthly_quota: i32,
    pub messages_used: i64,
    pub percentage_used: f64,
    pub remaining: i64,
    pub will_exceed: bool,
}

/// Get analytics dashboard
/// GET /api/v1/analytics/dashboard
pub async fn get_dashboard(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
) -> Result<Json<AnalyticsDashboard>, ApiError> {
    tracing::info!(api_key_id = %api_key.id, "Fetching analytics dashboard");

    // Get current month total
    let current_month = DailyUsageSummary::get_current_month_total(&state.db, api_key.id)
        .await
        .map_err(ApiError::from)?;

    // Get last 7 days
    let last_7_days = DailyUsageSummary::get_last_n_days(&state.db, api_key.id, 7)
        .await
        .map_err(ApiError::from)?;

    // Get last 4 weeks
    let last_4_weeks = WeeklyUsageSummary::get_last_n_weeks(&state.db, api_key.id, 4)
        .await
        .map_err(ApiError::from)?;

    // Calculate quota usage
    let messages_used = current_month.total_messages_sent;
    let monthly_quota = api_key.monthly_message_quota as i64;
    let percentage_used = (messages_used as f64 / monthly_quota as f64) * 100.0;
    let remaining = monthly_quota - messages_used;
    let will_exceed = remaining < (monthly_quota / 10); // Less than 10% remaining

    let quota_usage = QuotaUsage {
        monthly_quota: api_key.monthly_message_quota,
        messages_used,
        percentage_used,
        remaining,
        will_exceed,
    };

    // Get usage trends
    let trends = vaultless_core::models::usage_timescale::get_usage_trends(&state.db, api_key.id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(AnalyticsDashboard {
        current_month,
        last_7_days,
        last_4_weeks,
        quota_usage,
        trends,
    }))
}

/// Get daily usage for date range
/// GET /api/v1/analytics/daily?start=2025-01-01&end=2025-01-31
pub async fn get_daily_usage(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    axum::extract::Query(params): axum::extract::Query<DateRangeQuery>,
) -> Result<Json<Vec<DailyUsageSummary>>, ApiError> {
    let start = params.start.parse().map_err(|_| {
        ApiError::bad_request("Invalid start date format. Use ISO 8601 (YYYY-MM-DD)")
    })?;

    let end = params
        .end
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid end date format. Use ISO 8601 (YYYY-MM-DD)"))?;

    let summaries = DailyUsageSummary::get_range(&state.db, api_key.id, start, end)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(summaries))
}

/// Get weekly usage for date range
/// GET /api/v1/analytics/weekly?start=2025-01-01&end=2025-03-31
pub async fn get_weekly_usage(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    axum::extract::Query(params): axum::extract::Query<DateRangeQuery>,
) -> Result<Json<Vec<WeeklyUsageSummary>>, ApiError> {
    let start = params.start.parse().map_err(|_| {
        ApiError::bad_request("Invalid start date format. Use ISO 8601 (YYYY-MM-DD)")
    })?;

    let end = params
        .end
        .parse()
        .map_err(|_| ApiError::bad_request("Invalid end date format. Use ISO 8601 (YYYY-MM-DD)"))?;

    let summaries = WeeklyUsageSummary::get_range(&state.db, api_key.id, start, end)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(summaries))
}

#[derive(Debug, serde::Deserialize)]
pub struct DateRangeQuery {
    pub start: String,
    pub end: String,
}
