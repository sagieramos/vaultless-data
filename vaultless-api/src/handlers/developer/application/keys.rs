//! Key rotation handlers for applications.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use vaultless_core::models::Application;

use crate::{
    middleware::{error::ApiError, user::SessionDataUserExt},
    state::AppState,
};

// =============================================================================
// Response DTOs
// =============================================================================

/// Response for secret key rotation
#[derive(Debug, Serialize, ToSchema)]
pub struct RotateSecretKeyResponse {
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

/// Response for publishable key rotation
#[derive(Debug, Serialize, ToSchema)]
pub struct RotatePublishableKeyResponse {
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
pub struct AddPublishableKeyResponse {
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

// =============================================================================
// Request DTOs
// =============================================================================

/// Request to rotate a specific publishable key
#[derive(Debug, Deserialize, ToSchema)]
pub struct RotatePublishableKeyRequest {
    /// Optional: specific key ID to rotate. If not provided, rotates the oldest active key.
    #[schema(value_type = Option<String>)]
    pub key_id: Option<Uuid>,
}

// =============================================================================
// Handlers
// =============================================================================

/// Rotate an application's secret key
///
/// Creates a new secret key and deactivates the old one. The new key is only shown once.
/// All existing sessions using the old key will be invalidated.
#[utoipa::path(
    post,
    path = "/dev/applications/{app_id}/keys/secret/rotate",
    params(("app_id" = Uuid, Path, description = "Application ID")),
    responses(
        (status = 200, description = "Secret key rotated successfully", body = RotateSecretKeyResponse,
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
    security(("bearer_auth" = [])),
    tag = "applications"
)]
pub async fn rotate_secret_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<Json<RotateSecretKeyResponse>, ApiError> {
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

    Ok(Json(RotateSecretKeyResponse {
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
    params(("app_id" = Uuid, Path, description = "Application ID")),
    request_body(content = RotatePublishableKeyRequest, description = "Optional key ID to rotate"),
    responses(
        (status = 200, description = "Publishable key rotated successfully", body = RotatePublishableKeyResponse,
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
    security(("bearer_auth" = [])),
    tag = "applications"
)]
pub async fn rotate_publishable_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
    Json(req): Json<RotatePublishableKeyRequest>,
) -> Result<Json<RotatePublishableKeyResponse>, ApiError> {
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

    Ok(Json(RotatePublishableKeyResponse {
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
    params(("app_id" = Uuid, Path, description = "Application ID")),
    responses(
        (status = 200, description = "Publishable key added successfully", body = AddPublishableKeyResponse,
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
    security(("bearer_auth" = [])),
    tag = "applications"
)]
pub async fn add_publishable_key(
    State(state): State<AppState>,
    SessionDataUserExt(session): SessionDataUserExt,
    Path(app_id): Path<Uuid>,
) -> Result<Json<AddPublishableKeyResponse>, ApiError> {
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

    Ok(Json(AddPublishableKeyResponse {
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
    security(("bearer_auth" = [])),
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
