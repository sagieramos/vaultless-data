use std::collections::HashMap;

use axum::{
    Extension,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use vaultless_core::{
    models::{
        Application, ApplicationWithTier, CreateApplication, UpdateApplication,
    },
    types::SubscriptionTier,
};

use crate::{
    middleware::{application::OwnedApplication, error::ApiError},
    services::token::SessionData,
    state::AppState,
};

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

#[derive(Debug, Serialize)]
pub struct ApplicationResponse {
    pub application: ApplicationWithTier,
}

use vaultless_core::models::user::User;

// =============================================================================
// Handlers
// =============================================================================

/// Create a new application
/// POST /api/applications
pub async fn create_application(
    State(state): State<AppState>,
    Extension(user): Extension<SessionData>, // Extract authenticated user
    Json(req): Json<CreateApplicationRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let user_id = user.user_id;
    let user = User::find_by_id(&state.db, user_id)
        .await
        .map_err(ApiError::from)?;
    // Create application input
    let input = CreateApplication {
        user_id: user.id,
        name: req.name,
        description: req.description,
        max_ttl_seconds: None,

        is_key_rotation_forced: false,

        integrity_config:
    };

    // Create application
    let response = Application::create(&*state.db, input)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %user.id,
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

/// List user's applications with tier information
/// GET /api/applications
pub async fn list_applications(
    State(state): State<AppState>,
    Extension(user): Extension<SessionData>,
) -> Result<Json<ApplicationListResponse>, ApiError> {
    let applications = Application::find_by_user_with_tier(&*state.db, user.user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ApplicationListResponse {
        total: applications.len(),
        applications,
    }))
}

/// Get application by ID with tier info
/// GET /api/applications/:id
pub async fn get_application(
    State(state): State<AppState>,
    Extension(user): Extension<SessionData>,
    Path(app_id): Path<Uuid>,
) -> Result<Json<ApplicationResponse>, ApiError> {
    let app = Application::find_by_id_with_tier(&*state.db, app_id)
        .await
        .map_err(ApiError::from)?;

    // Verify ownership
    if app.user_id != user.user_id {
        return Err(ApiError::forbidden("You don't own this application").with_code("NOT_OWNER"));
    }

    Ok(Json(ApplicationResponse { application: app }))
}

/// Get application health check
/// GET /api/applications/:id/health
pub async fn get_application_health(
    Extension(OwnedApplication(app)): Extension<OwnedApplication>,
    State(state): State<AppState>,
) -> Result<Json<ApplicationHealth>, ApiError> {
    let health = app
        .health_check(&*state.db, Some(state.redis_pool.clone()))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(health))
}

/// Update application tier
/// PUT /api/applications/:id/tier
pub async fn update_application_tier(
    Extension(OwnedApplication(app)): Extension<OwnedApplication>,
    State(state): State<AppState>,
    Json(req): Json<UpdateTierRequest>,
) -> Result<Json<ApplicationResponse>, ApiError> {
    Application::update_tier(&*state.db, None, app.id, req.tier)
        .await
        .map_err(ApiError::from)?;

    let updated_app = Application::find_by_id_with_tier(&*state.db, app.id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ApplicationResponse {
        application: updated_app,
    }))
}

/// Update application metadata
/// PATCH /api/applications/:id
pub async fn update_application(
    State(state): State<AppState>,
    Extension(user): Extension<SessionData>,
    Path(app_id): Path<Uuid>,
    Json(req): Json<UpdateApplication>,
) -> Result<Json<ApplicationResponse>, ApiError> {
    // 1. Verify ownership
    let app = Application::find_by_id(&*state.db, app_id)
        .await
        .map_err(ApiError::from)?;

    if app.user_id != user.user_id {
        return Err(ApiError::forbidden("You don't own this application").with_code("NOT_OWNER"));
    }

    // 2. Perform update using model method
    let _ = Application::update(
        &*state.db,
        Some(state.redis_pool.clone()),
        app_id,
        UpdateApplication {
            name: req.name.clone(),
            description: req.description.clone(),
            bundle_id: req.bundle_id.clone(),
            platform: req.platform.clone(),
            webhook_url: req.webhook_url.clone(),
            is_active: req.is_active,
        },
    )
    .await
    .map_err(ApiError::from)?;

    // 3. Fetch application with tier info
    let app_with_tier = Application::find_by_id_with_tier(&*state.db, app_id)
        .await
        .map_err(ApiError::from)?;

    // 4. Return response
    Ok(Json(ApplicationResponse {
        application: app_with_tier,
    }))
}

/// Deactivate application
/// DELETE /api/applications/:id
pub async fn deactivate_application(
    State(state): State<AppState>,
    Extension(user): Extension<SessionData>,
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
    Application::deactivate(&*state.db, Some(state.redis_pool.clone()), app_id)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %user.user_id,
        application_id = %app_id,
        "Application deactivated"
    );

    Ok(StatusCode::NO_CONTENT)
}

/// Get application statistics (clients count, usage, etc.)
/// GET /api/applications/:id/stats
pub async fn get_application_stats(
    State(state): State<AppState>,
    Extension(user): Extension<SessionData>,
    Path(app_id): Path<Uuid>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !params.is_empty() {
        tracing::warn!(user = %user.user_id, "Rejected list_applications with unexpected query: {:?}", params);
        return Err(ApiError::bad_request("Unexpected query parameters")
            .with_code("UNEXPECTED_QUERY_PARAMS"));
    }
    // Verify ownership
    let app = Application::find_by_id(&*state.db, app_id)
        .await
        .map_err(ApiError::from)?;

    if app.user_id != user.user_id {
        return Err(ApiError::forbidden("You don't own this application").with_code("NOT_OWNER"));
    }

    // Get client count
    let client_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM clients WHERE application_id = $1 AND is_active = true",
    )
    .bind(app_id)
    .fetch_one(&*state.db)
    .await
    .map_err(ApiError::from)?;

    // Get usage metrics
    let usage = vaultless_core::get_aggregate_by_application_id(&*state.db, app_id)
        .await
        .map_err(ApiError::from)?;

    // Get current quota status
    let health = app
        .health_check(&*state.db, Some(state.redis_pool.clone()))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(serde_json::json!({
        "application_id": app_id,
        "active_clients": client_count,
        "usage": usage,
        "health": health,
    })))
}

/// Endpoint: GET /api/v1/applications/:app_id/realtime-usage
/// Retrieves real-time metrics for the current hour for a given application.
pub async fn get_app_realtime_usage(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    // Auth context (assumes user is authenticated and authorized for this app_id)
) -> Result<impl IntoResponse> {
    // 1. Fetch the real-time counters through the Application model.
    let counters = match Application::get_current_period_counters(
        pg_pool.as_ref(), // Pass the concrete PgPool for execution
        redis_pool,
        app_id,
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            error!(application_id = %app_id, "Failed to fetch real-time metrics: {:?}", e);
            // Return an appropriate error response
            return Err(e);
        }
    };

    // 2. Determine the period start time for context (client side needs this).
    // This uses the same logic as MetricKey generation for the current hour.
    let current_period_start =
        crate::usage_metrics::get_period_start(&chrono::Utc::now()).to_rfc3339();

    // 3. Construct and return the successful response
    let response = RealTimeUsageResponse {
        current_period_start_utc: current_period_start,
        counters,
    };

    Ok(Json(response).into_response())
}
