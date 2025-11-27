use axum::{
    extract::{FromRequestParts, Request, State},
    http::request::Parts,
    middleware::Next,
    response::Response,
};

use vaultless_core::models::user::User;

use crate::{
    middleware::error::ApiError, services::token::SessionData as SessionDataUser, state::AppState,
};

#[derive(Debug, Clone)]
pub struct SessionDataUserExt(pub SessionDataUser);

#[derive(Debug, Clone)]
pub struct UserExt(pub User);

pub async fn user_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = super::helper::extract_bearer_token(request.headers())?;

    let token_service = &state.token_service;
    let session_data = token_service.verify_access_token(&token).await?;

    request.extensions_mut().insert(session_data);
    Ok(next.run(request).await)
}

impl<S> FromRequestParts<S> for SessionDataUserExt
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<SessionDataUserExt>()
            .cloned()
            .ok_or_else(|| {
                ApiError::unauthorized("Missing session data").with_code("MISSING_SESSION_DATA")
            })
    }
}

impl FromRequestParts<AppState> for UserExt {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session_data = parts
            .extensions
            .get::<SessionDataUserExt>()
            .ok_or(ApiError::unauthorized("Missing session"))?;

        let user = User::find_by_id(&state.db, session_data.0.user_id)
            .await
            .map_err(ApiError::from)?;
        Ok(UserExt(user))
    }
}
