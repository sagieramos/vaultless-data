use uuid::Uuid;
use vaultless_core::ApiKey;
use crate::{
    middleware::error::ApiError,
    services::token::SessionData,
    state::AppState,
};

/// Validate that the given API key belongs to the user.
/// Returns the API key ID if valid.
pub async fn validate_api_key(
    state: &AppState,
    user_id_str: &str,
    api_key_id_str: &str,
) -> Result<(Uuid, String), ApiError> {
    // Parse user_id
    let user_id = Uuid::parse_str(user_id_str)
        .map_err(|_| ApiError::bad_request("Invalid user_id UUID"))?;

    // Parse api_key_id
    let api_key_id = Uuid::parse_str(api_key_id_str)
        .map_err(|_| ApiError::bad_request("Invalid api_key_id UUID"))?;

    // Fetch API key from DB
    let api_key = ApiKey::find_by_id(&state.db, api_key_id)
        .await
        .map_err(ApiError::from)?;

    // Check ownership
    if api_key.user_id != user_id {
        return Err(ApiError::forbidden("You don't own this API key"));
    }

    Ok((api_key.id, api_key.key_prefix)) 
}