use super::helper::*;
use axum::{extract::FromRequestParts, http::request::Parts};
use vaultless_core::models::session::HybridSessionVerifier;

use crate::{middleware::error::ApiError, state::AppState};
use vaultless_core::{Client, SessionData as SessionDataClient};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

#[derive(Debug, Clone)]
pub struct ClientExt(pub Client);

#[derive(Debug, Clone)]
pub struct SessionDataClientExt(pub SessionDataClient);

pub async fn client_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = extract_bearer_token(req.headers())?;

    let session_data = HybridSessionVerifier::verify_fast(&state.session_verifier, token)
        .await
        .map_err(ApiError::from)?;

    tracing::debug!(
        client_id = %session_data.client_id,
        device_trusted = session_data.device_trusted,
        "Session validated successfully"
    );

    req.extensions_mut()
        .insert(SessionDataClientExt(session_data));

    Ok(next.run(req).await)
}

// ADD THIS: FromRequestParts for SessionDataClientExt
impl FromRequestParts<AppState> for SessionDataClientExt {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<SessionDataClientExt>()
            .cloned()
            .ok_or_else(|| {
                ApiError::unauthorized("Missing session data").with_code("MISSING_SESSION_DATA")
            })
    }
}

impl FromRequestParts<AppState> for ClientExt {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session_data = parts
            .extensions
            .get::<SessionDataClientExt>()
            .ok_or(ApiError::unauthorized("Missing session"))?;

        let client =
            Client::fetch_active_client(&state.db, &state.redis_pool, session_data.0.client_id)
                .await
                .map_err(ApiError::from)?;

        Ok(ClientExt(client))
    }
}
