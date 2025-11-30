use super::helper::*;
use axum::{extract::FromRequestParts, http::request::Parts};

use crate::{middleware::error::ApiError, state::AppState};
use vaultless_core::ApplicationKeyView;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ApplicationKeyViewExt(pub Arc<ApplicationKeyView>);

pub async fn app_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let api_key = extract_api_key(req.headers())?;

    let auth_config =
        ApplicationKeyView::resolve_and_validate(&*state.db, state.redis_pool, api_key)
            .await
            .map_err(ApiError::from)?;

    let auth_config = Arc::new(auth_config);

    tracing::debug!(
        app_id = %auth_config.app_id,
        developer_id = %auth_config.app_user_id,
        "API key validated successfully"
    );

    req.extensions_mut()
        .insert(ApplicationKeyViewExt(auth_config));

    Ok(next.run(req).await)
}

impl FromRequestParts<AppState> for ApplicationKeyViewExt {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ApplicationKeyViewExt>()
            .cloned()
            .ok_or_else(|| {
                ApiError::unauthorized("Missing API key authentication")
                    .with_code("MISSING_API_KEY_AUTH")
            })
    }
}
