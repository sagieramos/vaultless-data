use async_trait::async_trait;
use axum::{
    extract::FromRequestParts,
    http::request::Parts,
};
use uuid::Uuid;
use crate::middleware::error::ApiError;
use crate::services::token::SessionService;

pub struct CurrentUser(pub Uuid);

#[async_trait]
impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Example: get session token from headers
        let token = parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| ApiError::unauthorized("Missing Authorization header"))?;

        // Use your SessionService to validate token
        let user_id_str = SessionService::validate_token(token)
            .await
            .map_err(|_| ApiError::unauthorized("Invalid token"))?;

        let user_id = Uuid::parse_str(&user_id_str)
            .map_err(|_| ApiError::unauthorized("Invalid user ID in token"))?;

        Ok(CurrentUser(user_id))
    }
}
