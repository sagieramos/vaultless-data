//! Application CRUD handlers.

use axum::{
    debug_handler,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;
use vaultless_core::{
    Decimal,
    models::{Application, CreateApplication, UpdateApplication, applications::dto::*},
};

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

use super::dto::{
    ApplicationResponse, CreateApplicationRequest, CreateApplicationResponse,
    PaginationParams, QuotaWarningsQuery,
};

// =============================================================================
// Create Application
// =============================================================================

/// Create a new application
#[utoipa::path(
    post,
    path = "/dev/applications",
    request_body = CreateApplicationRequest,
    responses(
        (status = 201, description = "Application created successfully", body = CreateApplicationResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "Conflict - application already exists"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "applications"
)]
pub async fn create_application(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<Json<CreateApplicationResponse>, ApiError> {
    let input = CreateApplication {
        user_id: session.user_id,
        name: req.name,
        description: req.description,
        max_ttl_seconds: None,
        is_key_rotation_forced: Some(false),
    };

    let response = Application::create(state.db, Some(state.redis_pool), input)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %session.user_id,
        application_id = %response.application.id,
        "Application created"
    );

    Ok(Json(CreateApplicationResponse {
        application: response.application.into(),
        secret_key: response.secret_key.unwrap_or_default(),
        publishable_key: response.publishable_key_plaintext,
        message: "IMPORTANT: Save your secret key now. You won't be able to see it again!"
            .to_string(),
    }))
}

// =============================================================================
// List Applications
// =============================================================================

/// List user's applications with tier information
#[utoipa::path(
    get,
    path = "/dev/applications",
    params(
        ("page" = Option<i64>, Query, description = "Page number (default: 1)"),
        ("page_size" = Option<i64>, Query, description = "Page size (default: 20)")
    ),
    responses(
        (status = 200, description = "List of applications retrieved successfully", body = PaginatedApplicationsSummary),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
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

// =============================================================================
// Update Application
// =============================================================================

/// Update application metadata
#[utoipa::path(
    patch,
    path = "/api/applications/{app_id}",
    params(("app_id" = Uuid, Path, description = "Application ID")),
    request_body = UpdateApplication,
    responses(
        (status = 200, description = "Application updated successfully", body = ApplicationResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
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

// =============================================================================
// Deactivate Application
// =============================================================================

/// Deactivate application
#[utoipa::path(
    delete,
    path = "/api/applications/{app_id}",
    params(("app_id" = Uuid, Path, description = "Application ID")),
    responses(
        (status = 204, description = "Application deactivated successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
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

// =============================================================================
// Get Application with Keys
// =============================================================================

/// Get application by ID including publishable keys and webhooks
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/with_keys",
    params(("application_id" = Uuid, Path, description = "Application ID")),
    responses(
        (status = 200, description = "Application with keys retrieved successfully", body = ApplicationWithUsage),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "applications"
)]
pub async fn get_application_with_keys(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(application_id): Path<Uuid>,
) -> Result<Json<ApplicationWithUsage>, ApiError> {
    // Use find_owned_by_user which returns ApplicationWithUsage (includes keys)
    let app =
        Application::find_owned_by_user(state.db.as_ref(), application_id, session.user_id)
            .await
            .map_err(ApiError::from)?;

    Ok(Json(app))
}

// =============================================================================
// Usage Summary & Quota Warnings
// =============================================================================

/// Returns aggregated usage statistics across all user's applications
#[utoipa::path(
    get,
    path = "/dev/applications/usage-summary",
    responses(
        (status = 200, description = "Usage summary retrieved successfully", body = UserUsageSummary),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
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

/// Returns applications that are approaching or exceeding their quota limits
#[utoipa::path(
    get,
    path = "/dev/applications/quota-warnings",
    params(QuotaWarningsQuery),
    responses(
        (status = 200, description = "Quota warnings retrieved successfully", body = PaginatedQuotaWarnings),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
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

// =============================================================================
// Dashboard Analytics
// =============================================================================

/// Full analytics endpoint for dashboards or heavy reporting
#[utoipa::path(
    get,
    path = "/dev/applications/{application_id}/analytics",
    params(("application_id" = Uuid, Path, description = "Application ID")),
    responses(
        (status = 200, description = "Application details with usage data", body = super::dto::ApplicationDashboardResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Application not found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "analytics"
)]
pub async fn get_application_analytics(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(application_id): Path<Uuid>,
) -> Result<Json<super::dto::ApplicationDashboardResponse>, ApiError> {
    let app_row =
        Application::find_owned_by_user(state.db.as_ref(), application_id, session.user_id)
            .await
            .map_err(ApiError::from)?;

    Ok(Json(app_row.into()))
}
