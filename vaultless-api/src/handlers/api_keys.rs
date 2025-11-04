use crate::{middleware::error::ApiError, services::token::SessionData, state::AppState};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;
use vaultless_core::getrandom;
use vaultless_core::{ApiKey, CreateApiKey, SubscriptionTier};
// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================
#[derive(Debug, Serialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub key_prefix: String,
    pub tier: String,
    pub description: Option<String>,
    pub scopes: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub monthly_quota: i32,
    pub rate_limit: i32,
}
#[derive(Debug, Deserialize, Validate)]
pub struct CreateApiKeyRequest {
    #[validate(length(min = 1, max = 255))]
    pub description: Option<String>,
    pub scopes: Option<String>,
    pub tier: Option<SubscriptionTier>,
}
#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub api_key: String, // The actual key (only shown once!)
    pub key_info: ApiKeyInfo,
    pub warning: String,
}
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateApiKeyRequest {
    #[validate(length(min = 1, max = 255))]
    pub description: Option<String>,
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
// ============================================================================
// HANDLERS
// ============================================================================
/// Create new API key
/// POST /api/v1/keys
pub async fn create_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), ApiError> {
    // Validate input
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    tracing::info!(
        user_id = %user_id,
        email = %session.email,
        "Creating new API key"
    );
    // Generate API key (vlt_live_<random_bytes>)
    let mut key_bytes = [0u8; 32];
    getrandom::fill(&mut key_bytes).map_err(|e| {
        ApiError::internal_server_error(format!("Failed to generate API key: {}", e))
    })?;
    let key_suffix = URL_SAFE_NO_PAD.encode(key_bytes);
    let api_key_string = format!("vlt_live_{}", key_suffix);
    // Hash the key for storage
    let key_hash = vaultless_core::crypto::hash_content(api_key_string.as_bytes());
    // Key prefix for display (first 8 chars after vlt_live_)
    let key_prefix = format!("vlt_live_{}", &key_suffix[..8]);
    // Create API key in database
    let tier = payload.tier.unwrap_or(SubscriptionTier::Free);
    let api_key = ApiKey::create(
        state.db.as_ref(),
        CreateApiKey {
            user_id,
            key_hash: key_hash.clone(),
            key_prefix: key_prefix.clone(),
            tier,
            description: payload.description.clone(),
            scopes: payload.scopes.clone(),
            expires_at: None, // No expiration by default
        },
    )
    .await
    .map_err(ApiError::from)?;
    tracing::info!(
        api_key_id = %api_key.id,
        tier = ?tier,
        "API key created successfully"
    );
    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            api_key: api_key_string, // Show the actual key ONLY once
            key_info: ApiKeyInfo {
                id: api_key.id.to_string(),
                key_prefix: api_key.key_prefix,
                tier: api_key.tier.to_string(),
                description: api_key.description,
                scopes: api_key.scopes,
                is_active: api_key.is_active,
                created_at: api_key.created_at.to_rfc3339(),
                last_used_at: api_key.last_used_at.map(|d| d.to_rfc3339()),
                expires_at: api_key.expires_at.map(|d| d.to_rfc3339()),
                monthly_quota: api_key.monthly_message_quota,
                rate_limit: api_key.rate_limit_per_minute,
            },
            warning: "Save this API key now. You won't be able to see it again!".to_string(),
        }),
    ))
}
/// List all API keys for current user
/// GET /api/v1/keys
pub async fn list_api_keys(
    State(state): State<AppState>,
    session: SessionData,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    tracing::debug!(user_id = %user_id, "Listing API keys");
    let paginated = ApiKey::find_by_owner(state.db.as_ref(), user_id, None, None)
        .await
        .map_err(ApiError::from)?;
    let keys = paginated.keys;
    let key_infos: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            id: k.id.to_string(),
            key_prefix: k.key_prefix,
            tier: k.tier.to_string(),
            description: k.description,
            scopes: k.scopes,
            is_active: k.is_active,
            created_at: k.created_at.to_rfc3339(),
            last_used_at: k.last_used_at.map(|d| d.to_rfc3339()),
            expires_at: k.expires_at.map(|d| d.to_rfc3339()),
            monthly_quota: k.monthly_message_quota,
            rate_limit: k.rate_limit_per_minute,
        })
        .collect();
    Ok(Json(key_infos))
}
/// Get specific API key details
/// GET /api/v1/keys/:key_id
pub async fn get_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Path(key_id): Path<Uuid>,
) -> Result<Json<ApiKeyInfo>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    let key = ApiKey::find_by_id(state.db.as_ref(), Some(state.redis_pool), key_id)
        .await
        .map_err(ApiError::from)?;
    // Verify ownership
    if key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }
    Ok(Json(ApiKeyInfo {
        id: key.id.to_string(),
        key_prefix: key.key_prefix,
        tier: key.tier.to_string(),
        description: key.description,
        scopes: key.scopes,
        is_active: key.is_active,
        created_at: key.created_at.to_rfc3339(),
        last_used_at: key.last_used_at.map(|d| d.to_rfc3339()),
        expires_at: key.expires_at.map(|d| d.to_rfc3339()),
        monthly_quota: key.monthly_message_quota,
        rate_limit: key.rate_limit_per_minute,
    }))
}
/// Update API key metadata
/// PATCH /api/v1/keys/:key_id
pub async fn update_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Path(key_id): Path<Uuid>,
    Json(payload): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyInfo>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    // Verify ownership
    let key = ApiKey::find_by_id(state.db.as_ref(), Some(state.redis_pool.clone()), key_id)
        .await
        .map_err(ApiError::from)?;
    if key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }
    // Update metadata
    let updated_key = ApiKey::update_metadata(
        state.db.as_ref(),
        Some(state.redis_pool),
        key_id,
        payload.description,
    )
    .await
    .map_err(ApiError::from)?;
    tracing::info!(key_id = %key_id, "API key metadata updated");
    Ok(Json(ApiKeyInfo {
        id: updated_key.id.to_string(),
        key_prefix: updated_key.key_prefix,
        tier: updated_key.tier.to_string(),
        description: updated_key.description,
        scopes: updated_key.scopes,
        is_active: updated_key.is_active,
        created_at: updated_key.created_at.to_rfc3339(),
        last_used_at: updated_key.last_used_at.map(|d| d.to_rfc3339()),
        expires_at: updated_key.expires_at.map(|d| d.to_rfc3339()),
        monthly_quota: updated_key.monthly_message_quota,
        rate_limit: updated_key.rate_limit_per_minute,
    }))
}
/// Revoke (delete) API key
/// DELETE /api/v1/keys/:key_id
pub async fn revoke_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    // Verify ownership
    let key = ApiKey::find_by_id(state.db.as_ref(), Some(state.redis_pool.clone()), key_id)
        .await
        .map_err(ApiError::from)?;
    if key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }
    // Delete the key
    ApiKey::delete(state.db.as_ref(), Some(state.redis_pool), key_id)
        .await
        .map_err(ApiError::from)?;
    tracing::warn!(key_id = %key_id, "API key revoked");
    Ok(StatusCode::NO_CONTENT)
}
/// Deactivate API key (soft delete)
/// POST /api/v1/keys/:key_id/deactivate
pub async fn deactivate_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    // Verify ownership
    let key = ApiKey::find_by_id(state.db.as_ref(), Some(state.redis_pool.clone()), key_id)
        .await
        .map_err(ApiError::from)?;
    if key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }
    ApiKey::deactivate(state.db.as_ref(), Some(state.redis_pool), key_id)
        .await
        .map_err(ApiError::from)?;
    tracing::info!(key_id = %key_id, "API key deactivated");
    Ok(StatusCode::NO_CONTENT)
}
/// Reactivate API key
/// POST /api/v1/keys/:key_id/reactivate
pub async fn reactivate_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    // Verify ownership
    let key = ApiKey::find_by_id(state.db.as_ref(), Some(state.redis_pool.clone()), key_id)
        .await
        .map_err(ApiError::from)?;
    if key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }
    ApiKey::reactivate(state.db.as_ref(), Some(state.redis_pool), key_id)
        .await
        .map_err(ApiError::from)?;
    tracing::info!(key_id = %key_id, "API key reactivated");
    Ok(StatusCode::NO_CONTENT)
}
/// Request tier upgrade (triggers Stripe flow)
/// POST /api/v1/keys/:key_id/upgrade
pub async fn upgrade_api_key(
    State(state): State<AppState>,
    session: SessionData,
    Path(key_id): Path<Uuid>,
    Json(payload): Json<UpgradeRequest>,
) -> Result<Json<UpgradeResponse>, ApiError> {
    let user_id: Uuid = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;
    // Verify ownership
    let key = ApiKey::find_by_id(state.db.as_ref(), Some(state.redis_pool), key_id)
        .await
        .map_err(ApiError::from)?;
    if key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }
    // Validate upgrade path
    let current_price = key.tier.monthly_price_cents().unwrap_or(0);
    let target_price = payload.target_tier.monthly_price_cents().unwrap_or(0);
    if target_price <= current_price {
        return Err(ApiError::bad_request("Can only upgrade to higher tiers"));
    }
    // TODO: Create Stripe checkout session
    tracing::info!(
        key_id = %key_id,
        current_tier = ?key.tier,
        target_tier = ?payload.target_tier,
        "Upgrade requested"
    );
    Ok(Json(UpgradeResponse {
        current_tier: key.tier.to_string(),
        target_tier: payload.target_tier.to_string(),
        monthly_price: payload.target_tier.monthly_price_cents(),
        checkout_url: None, // TODO: Stripe URL
        requires_payment: payload
            .target_tier
            .monthly_price_cents()
            .is_some_and(|p| p > 0),
    }))
}
