use axum::extract::{Json, State};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;
use vaultless_core::{ApiKey, Client, ClientAccessToken};

use crate::{middleware::error::ApiError, state::AppState};

// =======================================================
// REQUEST + RESPONSE STRUCTS
// =======================================================

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterClientRequest {
    #[validate(length(min = 1, max = 255))]
    pub identifier_hash: String, // Hash of client identifier (e.g., device fingerprint)

    #[validate(length(min = 1, max = 100))]
    pub client_type: String, // e.g., "desktop_app", "mobile_app"

    #[validate(length(equal = 64))]
    pub public_key: String, // 64-byte (Base64 or hex) encoded public key
}

#[derive(Debug, Serialize)]
pub struct RegisterClientResponse {
    pub access_token: String,
    pub expires_at: chrono::DateTime<Utc>,
}

// =======================================================
// HANDLER
// =======================================================

// POST /clients/register
pub async fn register_client(
    State(state): State<AppState>,
    Json(req): Json<RegisterClientRequest>,
) -> Result<Json<RegisterClientResponse>, ApiError> {
    // Create client (using hash from client-side)
    let client = Client::get_or_create_by_hash(
        &state.db,
        req.identifier_hash,
        Some(req.public_key),
    )
    .await?;

    // Generate access token (24-hour expiry)
    let (full_token, _token_record) = ClientAccessToken::create(
        &state.db, client.id, 24, // 24 hours
    )
    .await?;

    // Return token to client (NEVER the client.id)
    Ok(Json(RegisterClientResponse {
        access_token: full_token, // e.g., "vlt_client_abc123..."
        expires_at: Utc::now() + Duration::hours(24),
    }))
}
