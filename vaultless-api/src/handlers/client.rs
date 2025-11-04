use axum::{Json, extract::State};
use vaultless_core::{
    AuthenticateClientRequest, AuthenticateClientResponse, Client, RegisterClientRequest,
    RegisterClientResponse,
};
use crate::{
    middleware::error::ApiError,
    state::AppState,
};

#[axum::debug_handler]
pub async fn register_client(
    State(state): State<AppState>,
    Json(input): Json<RegisterClientRequest>,
) -> Result<Json<RegisterClientResponse>, ApiError> {
    let response = Client::register(
        &state.db, input, None, // developer_id (get from context if needed)
        None, // api_key_id (get from context if needed)
    )
    .await?;

    Ok(Json(response))
}

#[axum::debug_handler]
pub async fn authenticate_client(
    State(state): State<AppState>,
    Json(input): Json<AuthenticateClientRequest>,
) -> Result<Json<AuthenticateClientResponse>, ApiError> {
    let response = Client::authenticate(&state.db, input).await?;
    Ok(Json(response))
}

#[axum::debug_handler]
pub async fn logout_client(
    State(state): State<AppState>,
    AuthenticatedClient(client): AuthenticatedClient,
) -> Result<Json<serde_json::Value>, ApiError> {
    Client::revoke_session(&state.db, client.id).await?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Session revoked"
    })))
}
