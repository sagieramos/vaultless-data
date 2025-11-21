use super::helper::*;
use axum::http::HeaderMap;
use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{middleware::error::ApiError, state::AppState};
use vaultless_core::{AuthConfig, Client, SessionData};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

#[derive(Debug, Clone)]
pub struct AuthConfigExt(pub AuthConfig);

pub async fn api_key_auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let api_key = extract_api_key(&headers)?;

    let app = AuthConfig::resolve_and_validate(&*state.db, state.redis_pool, api_key).await?;

    req.extensions_mut().insert(AuthConfigExt(app));

    Ok(next.run(req).await)
}

#[derive(Debug, Clone)]
pub struct SessionDataExt(pub SessionData);

pub async fn client_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = extract_bearer_token(req.headers())?;

    let session_data =
        Client::verify_session_fast(state.redis_pool.clone(), &state.session_key_manager, token)
            .await
            .map_err(ApiError::from)?;

    req.extensions_mut().insert(SessionDataExt(session_data));

    Ok(next.run(req).await)
}

#[derive(Debug, Clone)]
pub struct ClientExt(pub Client);

impl FromRequestParts<AppState> for ClientExt {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session_data = parts
            .extensions
            .get::<SessionDataExt>()
            .ok_or(ApiError::unauthorized("Missing session"))?;

        let client =
            Client::fetch_active_client(&state.db, &state.redis_pool, session_data.0.client_id)
                .await
                .map_err(ApiError::from)?;

        Ok(ClientExt(client))
    }
}
