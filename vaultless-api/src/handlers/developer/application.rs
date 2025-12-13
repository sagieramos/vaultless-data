use axum::{
    debug_handler,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
use vaultless_core::Decimal;
use vaultless_core::{
    models::{
        Application, CreateApplication, UpdateApplication,
        app_model::{chart::*, dto::*},
        usage::MetricCounters,
        user::User,
    },
    types::SubscriptionTier,
};

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

#[derive(Deserialize)]
pub struct ChartQueryParams {
    granularity: String,
    metric: String,
    start: String,
    end: String,
}

// =============================================================================
// Request/Response DTOs
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateApplicationRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct RealTimeUsageResponse {
    pub current_period_start_utc: String,
    #[serde(flatten)]
    pub counters: MetricCounters,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTierRequest {
    pub tier: SubscriptionTier,
}

#[derive(Debug, Serialize)]
pub struct ApplicationListResponse {
    pub applications: Vec<ApplicationWithTier>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApplicationResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub max_ttl_seconds: i32,
    pub is_key_rotation_forced: bool,
    pub deletion_requested_at: Option<DateTime<Utc>>,
    pub internal_notes: Option<String>,
    pub integrity_config: Value,
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
            integrity_config: app.integrity_config,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct QuotaWarningsQuery {
    #[serde(default = "default_threshold")]
    pub threshold: Option<Decimal>,

    #[serde(default = "default_page")]
    pub page: i64,

    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_threshold() -> Option<Decimal> {
    Some(Decimal::from(80))
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    20
}

// =============================================================================
// Handlers
// =============================================================================

/// Create a new application
/// POST /api/applications
pub async fn create_application(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Create application input
    let input = CreateApplication {
        user_id: session.user_id,
        name: req.name,
        description: req.description,
        max_ttl_seconds: None,
        is_key_rotation_forced: Some(false),
        integrity_config: None,
    };

    // Create application
    let response = Application::create(state.db, Some(state.redis_pool), input)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %session.user_id,
        application_id = %response.application.id,
        "Application created"
    );

    // Return the response with keys (only shown once!)
    Ok(Json(serde_json::json!({
        "application": response.application,
        "secret_key": response.secret_key,
        "publishable_key": response.publishable_key_plaintext,
        "message": "IMPORTANT: Save your secret key now. You won't be able to see it again!"
    })))
}

#[derive(Deserialize)]
pub struct PaginationParams {
    page: Option<i64>,
    page_size: Option<i64>,
}

/// List user's applications with tier information
/// GET /api/applications
pub async fn list_applications(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 200);

    let paged = Application::list_user_applications(&*state.db, user.user_id, page, page_size)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(paged))
}

/// Get application by ID with tier info
/// GET /api/applications/:id
pub async fn get_application(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<Json<ApplicationWithUsageResponse>, ApiError> {
    let app = Application::find_owned_by_user(&*state.db, app_id, user.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(app))
}

/// Update application metadata
/// PATCH /api/applications/:id
pub async fn update_application(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
    Json(req): Json<UpdateApplication>,
) -> Result<Json<ApplicationResponse>, ApiError> {
    let updated_app = Application::update(
        Arc::clone(&state.db),
        Some(state.redis_pool.clone()),
        req,
        app_id,
        user.user_id,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(updated_app.into()))
}

/// Deactivate application
/// DELETE /api/applications/:id
pub async fn deactivate_application(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Verify ownership
    let app = Application::find_by_id(&*state.db, app_id)
        .await
        .map_err(ApiError::from)?;

    if app.user_id != user.user_id {
        return Err(ApiError::forbidden("You don't own this application").with_code("NOT_OWNER"));
    }

    // Deactivate
    Application::deactivate_weak(
        state.db,
        Some(state.redis_pool.clone()),
        app_id,
        user.user_id,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %user.user_id,
        application_id = %app_id,
        "Application deactivated"
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Get chart data for application by ID
/// GET /api/applications/:id/chart?granularity=daily&metric=messages&start=2023-01-01&end=2023-01-31
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
        &*state.db,
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

/// GET /api/v1/applications/usage-summary
///
/// Returns aggregated usage statistics across all user's applications.
pub async fn get_user_usage_summary(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
) -> Result<Json<UserUsageSummary>, ApiError> {
    let summary = Application::get_user_usage_summary(&state.db, user.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(summary))
}

/// GET /api/v1/applications/quota-warnings
///
/// Returns applications that are approaching or exceeding their quota limits.
///
/// Query params:
/// - threshold: Percentage threshold (default: 80)
/// - page: Page number (default: 1)
/// - page_size: Items per page (default: 20)
#[debug_handler]
pub async fn get_quota_warnings(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Query(params): Query<QuotaWarningsQuery>,
) -> Result<Json<PaginatedQuotaWarnings>, ApiError> {
    let warnings = Application::get_quota_warnings(
        &state.db,
        user.user_id,
        params.threshold,
        params.page,
        params.page_size,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(warnings))
}

/// Get application by ID including secret key ID, publishable keys, and webhooks.
///
/// Route: GET /api/applications/:id/with_keys
pub async fn get_application_with_keys_handler(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(application_id): Path<Uuid>,
) -> Result<Json<ApplicationWithKeys>, ApiError> {
    let user_id = session.user_id;

    let app_with_keys = Application::get_application_with_keys(&state.db, application_id, user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(app_with_keys))
}
