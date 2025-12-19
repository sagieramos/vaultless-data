use super::helper::*;
use axum::{extract::FromRequestParts, http::request::Parts};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{middleware::error::ApiError, state::AppState};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use vaultless_core::{Client, SessionData as SessionDataClient};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClientResponse {
    pub identifier: Option<String>,
    pub public_key: Option<String>,

    pub allow_anonymous_messages: bool,
    pub require_proof_verification: bool,
    pub is_active: bool,
    pub is_platform_attested: bool,

    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
}

impl From<Client> for ClientResponse {
    fn from(client: Client) -> Self {
        Self {
            identifier: client.identifier,
            public_key: client.public_key,

            allow_anonymous_messages: client.allow_anonymous_messages,
            require_proof_verification: client.require_proof_verification,
            is_active: client.is_active,
            is_platform_attested: client.is_platform_attested,

            last_seen_at: client.last_seen_at,
            last_message_at: client.last_message_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionDataClientExt(pub SessionDataClient);

pub async fn client_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = extract_bearer_token(req.headers())?;

    let session_data = state
        .session_verifier_hybrid
        .verify_fast(token)
        .await
        .map_err(ApiError::from)?;

    tracing::debug!(
        client_id = %session_data.client_id,
        device_trust_score = %session_data.device_trust_score,
        "Session validated successfully"
    );

    req.extensions_mut()
        .insert(SessionDataClientExt(session_data));

    Ok(next.run(req).await)
}

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

impl FromRequestParts<AppState> for ClientResponse {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session_data = parts
            .extensions
            .get::<SessionDataClientExt>()
            .ok_or_else(|| ApiError::unauthorized("Missing session"))?;

        let client =
            Client::fetch_active_client(&state.db, &state.redis_pool, session_data.0.client_id)
                .await
                .map_err(ApiError::from)?;

        Ok(ClientResponse::from(client))
    }
}
