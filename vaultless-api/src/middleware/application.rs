use super::client::AuthenticatedClient;
use super::error::ApiError;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::state::AppState;

use vaultless_core::{ApiKey, get_live_usage};

/// Middleware to check if the client's application has exceeded its quota
pub async fn check_quota(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // Extract the authenticated client from the request
    // This assumes the client was already authenticated by a previous middleware
    let client = match req.extensions().get::<AuthenticatedClient>() {
        Some(auth_client) => auth_client.0.clone(),
        None => {
            // If no authenticated client, skip quota check
            // (This should not happen if auth middleware runs first)
            tracing::warn!("No authenticated client found in request extensions");
            return Ok(next.run(req).await);
        }
    };

    // Get the api_key_id from the client
    let api_key_id = client.api_key_id.ok_or_else(|| {
        ApiError::internal_server_error("Client missing api_key_id").with_code("MISSING_API_KEY_ID")
    })?;

    // Fetch the API key
    let api_key = ApiKey::find_by_id(&*state.db, Some(state.redis_pool.clone()), api_key_id)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch API key {}: {}", api_key_id, e);
            ApiError::internal_server_error("Failed to verify quota")
                .with_code("QUOTA_CHECK_FAILED")
        })?;

    // Check if the key is active
    if !api_key.is_active {
        return Err(ApiError::forbidden("API key is inactive").with_code("API_KEY_INACTIVE"));
    }

    // Check if the key is expired
    if let Some(expires_at) = api_key.expires_at {
        if expires_at < chrono::Utc::now() {
            return Err(ApiError::forbidden("API key has expired").with_code("API_KEY_EXPIRED"));
        }
    }

    let quota_limit: i64 = api_key.monthly_message_quota.into();

    // Check quota - this fetches current usage once from Redis
    let quota_status = get_live_usage(state.redis_pool.clone(), api_key.id, quota_limit)
        .await
        .map_err(|e| {
            // Ensure you are using the correct variable name for tracing
            tracing::error!("Failed to get monthly usage for {}: {}", api_key.id, e);
            ApiError::internal_server_error("Failed to verify quota")
                .with_code("QUOTA_CHECK_FAILED")
        })?;

    // Compare with quota limit
    // FIX 2: Use the pre-calculated 'is_exceeded' field from the struct.
    if quota_status.is_exceeded {
        return Err(ApiError::forbidden(format!(
            // FIX 3: Use the .used and .limit fields from the struct
            "Monthly quota exceeded: {}/{} messages used",
            quota_status.used, quota_status.limit 
        ))
        .with_code("QUOTA_EXCEEDED"));
    }

    // Optional: Add warning header if approaching quota (>90%)
    // FIX 4: Use the pre-calculated 'percentage_used' field.
    if quota_status.percentage_used > 90.0 {
        tracing::warn!(
            api_key_id = %api_key.id,
            usage_percentage = quota_status.percentage_used,
            "API key approaching quota limit"
        );
        // You could add a custom header here if you want to warn clients
        // headers.insert("X-Quota-Warning", "Approaching limit");
    }

    // Quota check passed, continue to the handler
    Ok(next.run(req).await)
}
