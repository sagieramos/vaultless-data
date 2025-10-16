use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;
use vaultless_core::{ApiKey, CreateApiKey, SubscriptionTier};

use crate::{middleware::error::ApiError, services::RateLimiter, state::AppState};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateApiKeyRequest {
    #[validate(email)]
    pub owner_email: Option<String>,

    pub owner_name: Option<String>,
    pub organization: Option<String>,
    pub tier: Option<SubscriptionTier>,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub api_key: String,
    pub key_prefix: String,
    pub tier: String,
    pub monthly_quota: i32,
    pub warning: String,
}

/// Create a new API key (TEMPORARY - NO AUTH FOR TESTING)
/// POST /api/v1/admin/keys/create
pub async fn create_api_key(
    State(state): State<AppState>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreateApiKeyResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let tier = payload.tier.unwrap_or(SubscriptionTier::Free);

    // Generate API key
    let (api_key, key_hash) = vaultless_core::crypto::keys::generate_api_key("vlt_")
        .map_err(|e| ApiError::internal_server_error(format!("Key generation failed: {}", e)))?;

    // Get prefix (first 8 chars after vlt_)
    let key_prefix = api_key.chars().take(8).collect::<String>();

    // Create in database
    let created_key = ApiKey::create(
        &state.db,
        CreateApiKey {
            key_hash,
            key_prefix: key_prefix.clone(),
            tier,
            owner_email: payload.owner_email,
            owner_name: payload.owner_name,
            organization: payload.organization,
            expires_at: None,
            notes: Some("Created via admin endpoint".to_string()),
        },
    )
    .await
    .map_err(ApiError::from)?;

    tracing::warn!(
        "⚠️  API key created without authentication! Key ID: {}",
        created_key.id
    );

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            api_key: api_key.clone(),
            key_prefix,
            tier: tier.to_string(),
            monthly_quota: tier.default_monthly_quota(),
            warning: "SAVE THIS KEY NOW - IT WILL NOT BE SHOWN AGAIN".to_string(),
        }),
    ))
}

/// List all API keys (metadata only, no actual keys)
/// GET /api/v1/admin/keys
pub async fn list_api_keys(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKeyInfo>>, ApiError> {
    let keys = ApiKey::list(&state.db, 100, 0)
        .await
        .map_err(ApiError::from)?;

    let key_infos: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            id: k.id.to_string(),
            key_prefix: k.key_prefix,
            tier: k.tier.to_string(),
            owner_email: k.owner_email,
            is_active: k.is_active,
            created_at: k.created_at.to_rfc3339(),
            last_used_at: k.last_used_at.map(|d| d.to_rfc3339()),
        })
        .collect();

    Ok(Json(key_infos))
}

#[derive(Debug, Serialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub key_prefix: String,
    pub tier: String,
    pub owner_email: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// Get rate limit status for an API key
/// GET /api/v1/admin/keys/:key_id/rate-limit
pub async fn get_rate_limit_status(
    State(state): State<AppState>,
    Path(key_id): Path<Uuid>,
) -> Result<Json<RateLimitStatus>, ApiError> {
    let api_key = ApiKey::find_by_id(&state.db, key_id)
        .await
        .map_err(ApiError::from)?;

    let rate_limiter = RateLimiter::new(state.cache.clone());

    let usage = rate_limiter.get_current_usage(api_key.id).await?;
    let violations = rate_limiter.get_violation_count(api_key.id).await?;

    Ok(Json(RateLimitStatus {
        api_key_id: api_key.id.to_string(),
        rate_limit_per_minute: api_key.rate_limit_per_minute,
        current_usage: usage.requests_in_window,
        remaining: api_key.rate_limit_per_minute as i64 - usage.requests_in_window,
        violations_24h: violations,
        window_start: usage.window_start,
        window_end: usage.window_end,
    }))
}

/// Reset rate limit for an API key
/// POST /api/v1/admin/keys/:key_id/rate-limit/reset
pub async fn reset_rate_limit(
    State(state): State<AppState>,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let rate_limiter = RateLimiter::new(state.cache.clone());
    rate_limiter.reset_limit(key_id).await?;

    tracing::info!(key_id = %key_id, "Rate limit reset");

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct RateLimitStatus {
    pub api_key_id: String,
    pub rate_limit_per_minute: i32,
    pub current_usage: i64,
    pub remaining: i64,
    pub violations_24h: i64,
    pub window_start: u64,
    pub window_end: u64,
}
