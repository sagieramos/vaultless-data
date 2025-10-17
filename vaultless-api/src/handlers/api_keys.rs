use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;
use vaultless_core::{ApiKey, SubscriptionTier};

use crate::{middleware::error::ApiError, state::AppState};

/// List all API keys for current user
/// GET /api/v1/keys
pub async fn list_my_keys(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    // Get all keys for this user's email
    let owner_email = api_key
        .owner_email
        .ok_or_else(|| ApiError::bad_request("API key has no associated email"))?;

    let keys = ApiKey::find_by_owner(&state.db, &owner_email)
        .await
        .map_err(ApiError::from)?;

    let key_infos: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            id: k.id.to_string(),
            key_prefix: k.key_prefix,
            tier: k.tier.to_string(),
            owner_name: k.owner_name,
            organization: k.organization,
            is_active: k.is_active,
            created_at: k.created_at.to_rfc3339(),
            last_used_at: k.last_used_at.map(|d| d.to_rfc3339()),
            expires_at: k.expires_at.map(|d| d.to_rfc3339()),
            monthly_quota: k.monthly_message_quota,
            rate_limit: k.rate_limit_per_minute,
            notes: k.notes,
        })
        .collect();

    Ok(Json(key_infos))
}

/// Get specific API key details
/// GET /api/v1/keys/:key_id
pub async fn get_key_details(
    State(state): State<AppState>,
    Extension(current_key): Extension<ApiKey>,
    Path(key_id): Path<Uuid>,
) -> Result<Json<ApiKeyInfo>, ApiError> {
    let key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    // Verify ownership
    if key.owner_email != current_key.owner_email {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    Ok(Json(ApiKeyInfo {
        id: key.id.to_string(),
        key_prefix: key.key_prefix,
        tier: key.tier.to_string(),
        owner_name: key.owner_name,
        organization: key.organization,
        is_active: key.is_active,
        created_at: key.created_at.to_rfc3339(),
        last_used_at: key.last_used_at.map(|d| d.to_rfc3339()),
        expires_at: key.expires_at.map(|d| d.to_rfc3339()),
        monthly_quota: key.monthly_message_quota,
        rate_limit: key.rate_limit_per_minute,
        notes: key.notes,
    }))
}

/// Update API key metadata
/// PATCH /api/v1/keys/:key_id
pub async fn update_key(
    State(state): State<AppState>,
    Extension(current_key): Extension<ApiKey>,
    Path(key_id): Path<Uuid>,
    Json(payload): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyInfo>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    // Verify ownership
    let key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    if key.owner_email != current_key.owner_email {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Update metadata
    let updated_key = ApiKey::update_metadata(
        &state.db,
        key_id,
        payload.owner_name,
        payload.organization,
        payload.notes,
    )
    .await
    .map_err(ApiError::from)?;

    Ok(Json(ApiKeyInfo {
        id: updated_key.id.to_string(),
        key_prefix: updated_key.key_prefix,
        tier: updated_key.tier.to_string(),
        owner_name: updated_key.owner_name,
        organization: updated_key.organization,
        is_active: updated_key.is_active,
        created_at: updated_key.created_at.to_rfc3339(),
        last_used_at: updated_key.last_used_at.map(|d| d.to_rfc3339()),
        expires_at: updated_key.expires_at.map(|d| d.to_rfc3339()),
        monthly_quota: updated_key.monthly_message_quota,
        rate_limit: updated_key.rate_limit_per_minute,
        notes: updated_key.notes,
    }))
}

/// Deactivate API key
/// POST /api/v1/keys/:key_id/deactivate
pub async fn deactivate_key(
    State(state): State<AppState>,
    Extension(current_key): Extension<ApiKey>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Verify ownership
    let key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    if key.owner_email != current_key.owner_email {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Don't allow deactivating the current key
    if key.id == current_key.id {
        return Err(ApiError::bad_request(
            "Cannot deactivate the key you're currently using",
        ));
    }

    ApiKey::deactivate(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(key_id = %key_id, "API key deactivated");

    Ok(StatusCode::NO_CONTENT)
}

/// Reactivate API key
/// POST /api/v1/keys/:key_id/reactivate
pub async fn reactivate_key(
    State(state): State<AppState>,
    Extension(current_key): Extension<ApiKey>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Verify ownership
    let key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    if key.owner_email != current_key.owner_email {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    ApiKey::reactivate(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(key_id = %key_id, "API key reactivated");

    Ok(StatusCode::NO_CONTENT)
}

/// Delete API key
/// DELETE /api/v1/keys/:key_id
pub async fn delete_key(
    State(state): State<AppState>,
    Extension(current_key): Extension<ApiKey>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    // Verify ownership
    let key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    if key.owner_email != current_key.owner_email {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Don't allow deleting the current key
    if key.id == current_key.id {
        return Err(ApiError::bad_request(
            "Cannot delete the key you're currently using",
        ));
    }

    ApiKey::delete(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    tracing::warn!(key_id = %key_id, "API key deleted");

    Ok(StatusCode::NO_CONTENT)
}

/// Request tier upgrade (triggers Stripe flow)
/// POST /api/v1/keys/:key_id/upgrade
pub async fn request_upgrade(
    State(state): State<AppState>,
    Extension(current_key): Extension<ApiKey>,
    Path(key_id): Path<Uuid>,
    Json(payload): Json<UpgradeRequest>,
) -> Result<Json<UpgradeResponse>, ApiError> {
    // Verify ownership
    let key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    if key.owner_email != current_key.owner_email {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    // Validate upgrade path (compare monthly price as a proxy for tier ordering)
    if payload.target_tier.monthly_price_cents() <= key.tier.monthly_price_cents() {
        return Err(ApiError::bad_request("Can only upgrade to higher tiers"));
    }

    // TODO: Create Stripe checkout session
    // For now, return upgrade info
    Ok(Json(UpgradeResponse {
        current_tier: key.tier.to_string(),
        target_tier: payload.target_tier.to_string(),
        monthly_price: payload.target_tier.monthly_price_cents(),
        checkout_url: None, // TODO: Stripe URL
        requires_payment: payload.target_tier.monthly_price_cents().is_some(),
    }))
}

#[derive(Debug, Serialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub key_prefix: String,
    pub tier: String,
    pub owner_name: Option<String>,
    pub organization: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub monthly_quota: i32,
    pub rate_limit: i32,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateApiKeyRequest {
    #[validate(length(min = 1, max = 255))]
    pub owner_name: Option<String>,

    #[validate(length(min = 1, max = 255))]
    pub organization: Option<String>,

    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpgradeRequest {
    pub target_tier: SubscriptionTier,
}

#[derive(Debug, Serialize)]
pub struct UpgradeResponse {
    pub current_tier: String,
    pub target_tier: String,
    pub monthly_price: Option<i32>,
    pub checkout_url: Option<String>,
    pub requires_payment: bool,
}
