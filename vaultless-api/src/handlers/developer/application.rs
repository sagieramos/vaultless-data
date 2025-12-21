use axum::{
    debug_handler,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use vaultless_core::{Decimal, models::app_model::integrity::dto::IntegrityConfig};
use vaultless_core::{
    models::{
        Application, CreateApplication, UpdateApplication,
        app_model::{chart::*, dto::*},
        usage::MetricCounters,
    },
    types::SubscriptionTier,
};

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

#[derive(Deserialize, ToSchema)]
pub struct ChartQueryParams {
    #[schema(example = "daily")]
    granularity: String,
    #[schema(example = "messages")]
    metric: String,
    #[schema(example = "2023-01-01")]
    start: String,
    #[schema(example = "2023-01-31")]
    end: String,
}

// =============================================================================
// Request/Response DTOs
// =============================================================================

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApplicationRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct RealTimeUsageResponse {
    #[schema(example = "2023-01-01T00:00:00Z")]
    pub current_period_start_utc: String,
    #[serde(flatten)]
    pub counters: MetricCounters,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTierRequest {
    pub tier: SubscriptionTier,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
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

#[derive(Debug, Deserialize, ToSchema, IntoParams)]
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
// Handlers
// =============================================================================

/// Create a new application
#[utoipa::path(
    post,
    path = "/dev/applications",
    request_body = CreateApplicationRequest,
    responses(
        (status = 201, description = "Application created successfully", body = CreateApplicationResponse,
            example = json!({
                "application": {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "name": "My App",
                    "description": "A sample application",
                    "is_active": true,
                    "created_at": "2025-01-15T10:30:00Z",
                    "updated_at": "2025-01-15T10:30:00Z",
                    "max_ttl_seconds": 3600,
                    "is_key_rotation_forced": false,
                    "internal_notes": null,
                    "integrity_config": {}
                },
                "secret_key": "sk_live_abc123xyz789...",
                "publishable_key": "pk_live_def456uvw123...",
                "message": "IMPORTANT: Save your secret key now. You won't be able to see it again!"
            })
        ),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Conflict - application already exists"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn create_application(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<Json<CreateApplicationResponse>, ApiError> {
    // Create application input
    let input = CreateApplication {
        user_id: session.user_id,
        name: req.name,
        description: req.description,
        max_ttl_seconds: None,
        is_key_rotation_forced: Some(false),
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
    Ok(Json(CreateApplicationResponse {
        application: response.application.into(),
        secret_key: response.secret_key.unwrap_or_default(),
        publishable_key: response.publishable_key_plaintext,
        message: "IMPORTANT: Save your secret key now. You won't be able to see it again!"
            .to_string(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct PaginationParams {
    /// Page number (default: 1)
    #[schema(example = 1)]
    page: Option<i64>,

    /// Page size (default: 20)
    #[schema(example = 20)]
    page_size: Option<i64>,
}

/// List user's applications with tier information
///
/// This endpoint supports ETag caching. Include the `If-None-Match` header with
/// a previously received ETag to get a 304 Not Modified response if data hasn't changed.
#[utoipa::path(
    get,
    path = "/dev/applications",
    params(
        ("page" = Option<i64>, Query, description = "Page number (default: 1)"),
        ("page_size" = Option<i64>, Query, description = "Page size (default: 20)"),
        ("If-None-Match" = Option<String>, Header, description = "ETag from previous response for conditional request")
    ),
    responses(
        (status = 200, description = "List of applications retrieved successfully", body = PaginatedApplicationsSummary,
            headers(
                ("ETag" = String, description = "Entity tag for cache validation. Use this in If-None-Match header for subsequent requests."),
                ("Cache-Control" = String, description = "Cache directives (e.g., public, max-age=60)")
            ),
            example = json!({
                "data": [
                    {
                        "application_id": "550e8400-e29b-41d4-a716-446655440000",
                        "name": "My App",
                        "description": "A sample application",
                        "is_active": true,
                        "created_at": "2025-01-15T10:30:00Z",
                        "updated_at": "2025-01-15T10:30:00Z",
                        "tier": "pro",
                        "monthly_message_quota": 100000,
                        "publishable_key_count": 2,
                        "webhook_count": 1,
                        "quota_usage_percentage": 45.5
                    }
                ],
                "total_count": 1,
                "page": 1,
                "page_size": 20,
                "total_pages": 1
            })
        ),
        (status = 304, description = "Not Modified - Data hasn't changed since last request. Use cached data."),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn list_applications(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedApplicationsSummary>, ApiError> {
    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(20).clamp(1, 200);

    let paged =
        Application::list_user_applications(state.db.as_ref(), user.user_id, page, page_size)
            .await
            .map_err(ApiError::from)?;

    Ok(Json(paged))
}

/// Update application metadata
#[utoipa::path(
    patch,
    path = "/api/applications/{app_id}",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    request_body = UpdateApplication,
    responses(
        (status = 200, description = "Application updated successfully", body = ApplicationResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
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
#[utoipa::path(
    delete,
    path = "/api/applications/{app_id}",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 204, description = "Application deactivated successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn deactivate_application(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
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
    security(
        ("bearer_auth" = [])
    ),
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

/// Returns aggregated usage statistics across all user's applications.
///
/// This endpoint supports ETag caching. Include the `If-None-Match` header with
/// a previously received ETag to get a 304 Not Modified response if data hasn't changed.
#[utoipa::path(
    get,
    path = "/dev/applications/usage-summary",
    params(
        ("If-None-Match" = Option<String>, Header, description = "ETag from previous response for conditional request")
    ),
    responses(
        (status = 200, description = "Usage summary retrieved successfully", body = UserUsageSummary,
            headers(
                ("ETag" = String, description = "Entity tag for cache validation"),
                ("Cache-Control" = String, description = "Cache directives (e.g., public, max-age=60)")
            )
        ),
        (status = 304, description = "Not Modified - Data hasn't changed since last request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn get_user_usage_summary(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
) -> Result<Json<UserUsageSummary>, ApiError> {
    let summary = Application::get_user_usage_summary(&state.db, user.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(summary))
}

/// Returns applications that are approaching or exceeding their quota limits.
///
/// This endpoint supports ETag caching. Include the `If-None-Match` header with
/// a previously received ETag to get a 304 Not Modified response if data hasn't changed.
#[utoipa::path(
    get,
    path = "/dev/applications/quota-warnings",
    params(
        QuotaWarningsQuery,
        ("If-None-Match" = Option<String>, Header, description = "ETag from previous response for conditional request")
    ),
    responses(
        (status = 200, description = "Quota warnings retrieved successfully", body = PaginatedQuotaWarnings,
            headers(
                ("ETag" = String, description = "Entity tag for cache validation"),
                ("Cache-Control" = String, description = "Cache directives (e.g., public, max-age=60)")
            )
        ),
        (status = 304, description = "Not Modified - Data hasn't changed since last request"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
#[debug_handler]
pub async fn get_quota_warnings(
    State(state): State<AppState>,
    SessionDataUserExt(user): SessionDataUserExt,
    Query(params): Query<QuotaWarningsQuery>,
) -> Result<Json<PaginatedQuotaWarnings>, ApiError> {
    let threshold = params
        .threshold
        .and_then(|t| Decimal::from_str(&t.to_string()).ok());

    let warnings = Application::get_quota_warnings(
        &state.db,
        user.user_id,
        threshold,
        params.page,
        params.page_size,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(warnings))
}

/// Get application by ID including publishable keys and webhooks.
///
/// This endpoint supports ETag caching. Include the `If-None-Match` header with
/// a previously received ETag to get a 304 Not Modified response if data hasn't changed.
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/with_keys",
    params(
        ("application_id" = Uuid, Path, description = "Application ID"),
        ("If-None-Match" = Option<String>, Header, description = "ETag from previous response for conditional request")
    ),
    responses(
        (status = 200, description = "Application with keys retrieved successfully", body = ApplicationWithKeys,
            headers(
                ("ETag" = String, description = "Entity tag for cache validation"),
                ("Cache-Control" = String, description = "Cache directives (e.g., public, max-age=60)")
            )
        ),
        (status = 304, description = "Not Modified - Data hasn't changed since last request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
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

// =============================================================================
// Key Rotation Handlers
// =============================================================================

/// Response for key rotation operations
#[derive(Debug, Serialize, ToSchema)]
pub struct RotateSecretKeyApiResponse {
    /// The application ID
    #[schema(value_type = String)]
    pub application_id: Uuid,
    /// The new secret key (only shown once, store securely!)
    #[schema(example = "sk_live_abc123xyz...")]
    pub new_secret_key: String,
    /// Prefix of the new key for identification
    #[schema(example = "sk_live_")]
    pub key_prefix: String,
    /// When the new key was created
    #[schema(value_type = String)]
    pub created_at: DateTime<Utc>,
    /// ID of the old key that was deactivated (for audit purposes)
    #[schema(value_type = String)]
    pub old_key_id: Uuid,
    /// Important message about saving the new key
    #[schema(example = "IMPORTANT: Save your new secret key now. You won't be able to see it again!")]
    pub message: String,
}

/// Response for publishable key rotation operations
#[derive(Debug, Serialize, ToSchema)]
pub struct RotatePublishableKeyApiResponse {
    /// The application ID
    #[schema(value_type = String)]
    pub application_id: Uuid,
    /// The new publishable key
    #[schema(example = "pk_live_def456uvw...")]
    pub new_publishable_key: String,
    /// Prefix of the new key for identification
    #[schema(example = "pk_live_def456uvw")]
    pub key_prefix: String,
    /// When the new key was created
    #[schema(value_type = String)]
    pub created_at: DateTime<Utc>,
    /// ID of the old key that was deactivated (for audit purposes)
    #[schema(value_type = String)]
    pub old_key_id: Uuid,
}

/// Response for adding a publishable key
#[derive(Debug, Serialize, ToSchema)]
pub struct AddPublishableKeyApiResponse {
    /// The application ID
    #[schema(value_type = String)]
    pub application_id: Uuid,
    /// The new publishable key
    #[schema(example = "pk_live_def456uvw...")]
    pub new_publishable_key: String,
    /// Prefix of the new key for identification
    #[schema(example = "pk_live_def456uvw")]
    pub key_prefix: String,
    /// When the new key was created
    #[schema(value_type = String)]
    pub created_at: DateTime<Utc>,
    /// Total number of active publishable keys for this application
    #[schema(example = 2)]
    pub total_active_publishable_keys: i64,
}

/// Request to rotate a specific publishable key
#[derive(Debug, Deserialize, ToSchema)]
pub struct RotatePublishableKeyRequest {
    /// Optional: specific key ID to rotate. If not provided, rotates the oldest active key.
    #[schema(value_type = Option<String>)]
    pub key_id: Option<Uuid>,
}

/// Request to deactivate a specific publishable key
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeactivatePublishableKeyRequest {
    /// The specific key ID to deactivate
    #[schema(value_type = String)]
    pub key_id: Uuid,
}

/// Rotate an application's secret key
///
/// Creates a new secret key and deactivates the old one. The new key is only shown once.
/// All existing sessions using the old key will be invalidated.
#[utoipa::path(
    post,
    path = "/dev/applications/{app_id}/keys/secret/rotate",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Secret key rotated successfully", body = RotateSecretKeyApiResponse,
            example = json!({
                "application_id": "550e8400-e29b-41d4-a716-446655440000",
                "new_secret_key": "sk_live_abc123xyz...",
                "key_prefix": "sk_live_",
                "created_at": "2025-01-15T10:30:00Z",
                "old_key_id": "660e8400-e29b-41d4-a716-446655440001",
                "message": "IMPORTANT: Save your new secret key now. You won't be able to see it again!"
            })
        ),
        (status = 400, description = "Bad request - application inactive"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application or key not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn rotate_secret_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<Json<RotateSecretKeyApiResponse>, ApiError> {
    let result = Application::rotate_secret_key(
        state.db,
        Some(state.redis_pool),
        app_id,
        session.user_id,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %session.user_id,
        application_id = %app_id,
        old_key_id = %result.old_key_id,
        "Secret key rotated"
    );

    Ok(Json(RotateSecretKeyApiResponse {
        application_id: result.application_id,
        new_secret_key: result.new_secret_key,
        key_prefix: result.key_prefix,
        created_at: result.created_at,
        old_key_id: result.old_key_id,
        message: "IMPORTANT: Save your new secret key now. You won't be able to see it again!"
            .to_string(),
    }))
}

/// Rotate an application's publishable key
///
/// Creates a new publishable key and deactivates the specified (or oldest) one.
/// Use this for gradual key rotation - create new key first, then migrate clients.
#[utoipa::path(
    post,
    path = "/dev/applications/{app_id}/keys/publishable/rotate",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    request_body(content = RotatePublishableKeyRequest, description = "Optional key ID to rotate"),
    responses(
        (status = 200, description = "Publishable key rotated successfully", body = RotatePublishableKeyApiResponse,
            example = json!({
                "application_id": "550e8400-e29b-41d4-a716-446655440000",
                "new_publishable_key": "pk_live_def456uvw...",
                "key_prefix": "pk_live_def456uvw",
                "created_at": "2025-01-15T10:30:00Z",
                "old_key_id": "770e8400-e29b-41d4-a716-446655440002"
            })
        ),
        (status = 400, description = "Bad request - application inactive"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application or key not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn rotate_publishable_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
    Json(req): Json<RotatePublishableKeyRequest>,
) -> Result<Json<RotatePublishableKeyApiResponse>, ApiError> {
    let result = Application::rotate_publishable_key(
        state.db,
        Some(state.redis_pool),
        app_id,
        session.user_id,
        req.key_id,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %session.user_id,
        application_id = %app_id,
        old_key_id = %result.old_key_id,
        "Publishable key rotated"
    );

    Ok(Json(RotatePublishableKeyApiResponse {
        application_id: result.application_id,
        new_publishable_key: result.new_publishable_key,
        key_prefix: result.key_prefix,
        created_at: result.created_at,
        old_key_id: result.old_key_id,
    }))
}

/// Add an additional publishable key to an application
///
/// Creates a new publishable key without deactivating existing ones.
/// Useful for multi-environment deployments or gradual migration.
/// Maximum 5 active publishable keys per application.
#[utoipa::path(
    post,
    path = "/dev/applications/{app_id}/keys/publishable",
    params(
        ("app_id" = Uuid, Path, description = "Application ID")
    ),
    responses(
        (status = 200, description = "Publishable key added successfully", body = AddPublishableKeyApiResponse,
            example = json!({
                "application_id": "550e8400-e29b-41d4-a716-446655440000",
                "new_publishable_key": "pk_live_ghi789rst...",
                "key_prefix": "pk_live_ghi789rst",
                "created_at": "2025-01-15T10:30:00Z",
                "total_active_publishable_keys": 2
            })
        ),
        (status = 400, description = "Bad request - maximum keys reached or application inactive"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn add_publishable_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<Json<AddPublishableKeyApiResponse>, ApiError> {
    let result = Application::add_publishable_key(
        state.db,
        Some(state.redis_pool),
        app_id,
        session.user_id,
        None, // Use default max keys (5)
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %session.user_id,
        application_id = %app_id,
        total_active_publishable_keys = result.total_active_publishable_keys,
        "Publishable key added"
    );

    Ok(Json(AddPublishableKeyApiResponse {
        application_id: result.application_id,
        new_publishable_key: result.new_publishable_key,
        key_prefix: result.key_prefix,
        created_at: result.created_at,
        total_active_publishable_keys: result.total_active_publishable_keys,
    }))
}

/// Deactivate a specific publishable key
///
/// Deactivates the specified publishable key without creating a new one.
/// Cannot deactivate the last active publishable key - use rotate instead.
#[utoipa::path(
    delete,
    path = "/dev/applications/{app_id}/keys/publishable/{key_id}",
    params(
        ("app_id" = Uuid, Path, description = "Application ID"),
        ("key_id" = Uuid, Path, description = "Publishable key ID to deactivate")
    ),
    responses(
        (status = 204, description = "Publishable key deactivated successfully"),
        (status = 400, description = "Bad request - cannot deactivate last key"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application or key not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "applications"
)]
pub async fn deactivate_publishable_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path((app_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    Application::deactivate_publishable_key(
        state.db,
        Some(state.redis_pool),
        app_id,
        session.user_id,
        key_id,
    )
    .await
    .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %session.user_id,
        application_id = %app_id,
        deactivated_key_id = %key_id,
        "Publishable key deactivated"
    );

    Ok(StatusCode::NO_CONTENT)
}
