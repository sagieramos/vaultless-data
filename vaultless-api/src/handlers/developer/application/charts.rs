//! Chart data handlers for applications.

use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;
use vaultless_core::models::{Application, app_model::chart::*};

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

use super::dto::ChartQueryParams;

/// Get chart data for application by ID
#[utoipa::path(
    get,
    path = "/api/applications/{app_id}/chart",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("granularity" = String, Query, description = "Aggregation granularity (daily, weekly)"),
        ("metric" = String, Query, description = "Metric to chart (messages, bandwidth, storage, proofs, rate_limits, cost, all)"),
        ("start" = String, Query, description = "Start date in YYYY-MM-DD format"),
        ("end" = String, Query, description = "End date in YYYY-MM-DD format")
    ),
    responses(
        (status = 200, description = "Chart data retrieved successfully", body = ApplicationChartData),
        (status = 400, description = "Invalid query parameters"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "applications"
)]
pub async fn get_chart_data(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
    Query(params): Query<ChartQueryParams>,
) -> Result<Json<ApplicationChartData>, ApiError> {
    let granularity = params
        .granularity
        .as_str()
        .parse::<ChartGranularity>()
        .map_err(|_| {
            ApiError::bad_request("Invalid granularity. Must be 'daily' or 'weekly'.")
                .with_code("INVALID_GRANULARITY")
        })?;

    let metric = params
        .metric
        .as_str()
        .parse::<ChartMetric>()
        .map_err(|_| {
            ApiError::bad_request("Invalid metric. Must be one of: messages, bandwidth, storage, proofs, rate_limits, cost, all.")
                .with_code("INVALID_METRIC")
        })?;

    let start_date = NaiveDate::parse_from_str(&params.start, "%Y-%m-%d").map_err(|_| {
        ApiError::bad_request("Invalid start date. Use YYYY-MM-DD format.")
            .with_code("INVALID_START_DATE")
    })?;

    let end_date = NaiveDate::parse_from_str(&params.end, "%Y-%m-%d").map_err(|_| {
        ApiError::bad_request("Invalid end date. Use YYYY-MM-DD format.")
            .with_code("INVALID_END_DATE")
    })?;

    let start_ts = DateTime::<Utc>::from_naive_utc_and_offset(
        start_date.and_hms_opt(0, 0, 0).ok_or_else(|| {
            ApiError::bad_request("Invalid start date").with_code("INVALID_START_DATE")
        })?,
        Utc,
    );

    let end_ts = DateTime::<Utc>::from_naive_utc_and_offset(
        end_date.and_hms_opt(23, 59, 59).ok_or_else(|| {
            ApiError::bad_request("Invalid end date").with_code("INVALID_END_DATE")
        })?,
        Utc,
    );

    if start_ts >= end_ts {
        return Err(ApiError::bad_request("Start date must be before end date.")
            .with_code("INVALID_DATE_RANGE"));
    }

    const MAX_DAILY_BUCKETS: u64 = 100;
    const MAX_WEEKLY_BUCKETS: u64 = 160;

    let range_days = (end_ts - start_ts).num_days() as u64;
    let max_buckets = match granularity {
        ChartGranularity::Daily => MAX_DAILY_BUCKETS,
        ChartGranularity::Weekly => MAX_WEEKLY_BUCKETS,
    };

    let estimated_buckets = match granularity {
        ChartGranularity::Daily => range_days,
        ChartGranularity::Weekly => (range_days as f64 / 7.0).ceil() as u64,
    };

    if estimated_buckets > max_buckets {
        return Err(ApiError::bad_request(format!(
            "Date range too large for {} granularity (max {} buckets). Try a shorter range.",
            granularity.as_api_str(),
            max_buckets
        ))
        .with_code("RANGE_TOO_LARGE"));
    }

    let chart_data = Application::get_chart_data(
        state.db.as_ref(),
        app_id,
        user.user_id,
        granularity,
        metric,
        start_ts,
        end_ts,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(chart_data))
}
