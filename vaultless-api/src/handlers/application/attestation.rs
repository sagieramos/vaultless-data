
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

use crate::middleware::error::{Result, ApiError};
use crate::models::app_model::attestation::{
    AttestationRequest, AttestationService, Platform,
    check_attestation_rate_limit, track_failed_attestation,
};
use crate::models::app_model::Application;

// =============================================================================
// SHARED STATE
// =============================================================================

#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub redis_pool: deadpool_redis::Pool,
    pub attestation_service: Arc<AttestationService>,
}

// =============================================================================
// REQUEST/RESPONSE TYPES
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct VerifyAttestationRequest {
    pub attestation: AttestationRequest,
}

#[derive(Debug, Serialize)]
pub struct AttestationResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChallengeResponse {
    pub challenge: String,
    pub expires_in: u64,
    pub platform: Platform,
}

// =============================================================================
// ATTESTATION VERIFICATION HANDLER
// =============================================================================

/// POST /api/v1/applications/:app_id/attestation/verify
/// Verify platform attestation (iOS, Android, or IoT)
pub async fn verify_attestation(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<VerifyAttestationRequest>,
) -> Result<Response> {
    let request = payload.attestation;

    // 1. Load application from database
    let app = Application::find_by_id(&state.db_pool, app_id)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

    // 2. Check if unauthenticated access is allowed (dev mode)
    if app.allows_unauthenticated() {
        return Ok((
            StatusCode::OK,
            Json(AttestationResponse {
                success: true,
                message: "Attestation bypassed (dev mode enabled)".to_string(),
                warnings: Some(vec![
                    "Application allows unauthenticated access".to_string(),
                ]),
                verdict: Some("DEV_MODE".to_string()),
            }),
        )
            .into_response());
    }

    // 3. Check if attestation is required for this platform
    if !app.requires_attestation(request.platform) {
        return Ok((
            StatusCode::OK,
            Json(AttestationResponse {
                success: true,
                message: format!(
                    "Attestation not configured for {} platform",
                    request.platform
                ),
                warnings: None,
                verdict: Some("NOT_REQUIRED".to_string()),
            }),
        )
            .into_response());
    }

    // 4. Rate limiting - check attestation attempts
    let rate_limit = app.get_attestation_rate_limit(request.platform);
    
    if let Err(e) = check_attestation_rate_limit(
        &state.redis_pool,
        &request.device_id,
        request.platform,
        rate_limit,
    )
    .await
    {
        tracing::warn!(
            app_id = %app_id,
            device_id = %request.device_id,
            platform = %request.platform,
            "Rate limit exceeded for attestation"
        );

        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            Json(AttestationResponse {
                success: false,
                message: e.to_string(),
                warnings: None,
                verdict: Some("RATE_LIMIT_EXCEEDED".to_string()),
            }),
        )
            .into_response());
    }

    // 5. Perform attestation verification
    let integrity_config = serde_json::to_value(&app.get_integrity_config()?)
        .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

    let result = state
        .attestation_service
        .verify_attestation(&request, &integrity_config)
        .await;

    // 6. Handle verification result
    match result {
        Ok(attestation_result) => {
            if !attestation_result.is_valid {
                // Track failed attempt
                let max_failures = app.get_max_failed_attempts();
                let _ = track_failed_attestation(
                    &state.redis_pool,
                    &request.device_id,
                    max_failures,
                )
                .await;

                tracing::warn!(
                    app_id = %app_id,
                    device_id = %request.device_id,
                    platform = %request.platform,
                    verdict = ?attestation_result.verdict,
                    error = ?attestation_result.error,
                    "Attestation verification failed"
                );

                return Ok((
                    StatusCode::UNAUTHORIZED,
                    Json(AttestationResponse {
                        success: false,
                        message: attestation_result
                            .error
                            .unwrap_or_else(|| "Attestation verification failed".to_string()),
                        warnings: attestation_result.warnings,
                        verdict: attestation_result.verdict,
                    }),
                )
                    .into_response());
            }

            // Success!
            tracing::info!(
                app_id = %app_id,
                device_id = %request.device_id,
                platform = %request.platform,
                bundle_id = %attestation_result.bundle_id,
                device_trusted = attestation_result.device_trusted,
                "Attestation verified successfully"
            );

            Ok((
                StatusCode::OK,
                Json(AttestationResponse {
                    success: true,
                    message: format!(
                        "Attestation verified successfully for {} ({})",
                        request.platform,
                        attestation_result.bundle_id
                    ),
                    warnings: attestation_result.warnings,
                    verdict: attestation_result.verdict,
                }),
            )
                .into_response())
        }
        Err(e) => {
            tracing::error!(
                app_id = %app_id,
                device_id = %request.device_id,
                platform = %request.platform,
                error = %e,
                "Attestation verification error"
            );

            // Don't leak internal errors to client
            Ok((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AttestationResponse {
                    success: false,
                    message: "Attestation verification failed".to_string(),
                    warnings: None,
                    verdict: Some("ERROR".to_string()),
                }),
            )
                .into_response())
        }
    }
}

// =============================================================================
// CHALLENGE GENERATION HANDLERS
// =============================================================================

/// GET /api/v1/applications/:app_id/attestation/challenge/ios
/// Generate iOS App Attest challenge
pub async fn generate_ios_challenge(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> Result<Response> {
    let app = Application::find_by_id(&state.db_pool, app_id)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

    // Check if iOS attestation is configured
    if !app.requires_attestation(Platform::IOS) {
        return Err(VaultlessError::IntegrityCheckFailed(
            "iOS attestation not configured for this application".into(),
        ));
    }

    let integrity_config = serde_json::to_value(&app.get_integrity_config()?)
        .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

    let challenge = state
        .attestation_service
        .generate_ios_challenge(&integrity_config)
        .await?;

    let ios_config = app.get_ios_config()?;

    tracing::debug!(
        app_id = %app_id,
        challenge_length = challenge.len(),
        "iOS challenge generated"
    );

    Ok((
        StatusCode::OK,
        Json(ChallengeResponse {
            challenge,
            expires_in: ios_config.challenge_ttl_seconds,
            platform: Platform::IOS,
        }),
    )
        .into_response())
}

/// GET /api/v1/applications/:app_id/attestation/challenge/iot
/// Generate IoT device challenge
pub async fn generate_iot_challenge(
    State(state): State<AppState>,
    Path(app_id): Path<Uuid>,
) -> Result<Response> {
    let app = Application::find_by_id(&state.db_pool, app_id)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

    // Check if IoT attestation is configured
    if !app.requires_attestation(Platform::IoT) {
        return Err(VaultlessError::IntegrityCheckFailed(
            "IoT attestation not configured for this application".into(),
        ));
    }

    let integrity_config = serde_json::to_value(&app.get_integrity_config()?)
        .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

    let challenge = state
        .attestation_service
        .generate_iot_challenge(&integrity_config)
        .await?;

    let iot_config = app.get_iot_config()?;

    tracing::debug!(
        app_id = %app_id,
        challenge_length = challenge.len(),
        "IoT challenge generated"
    );

    Ok((
        StatusCode::OK,
        Json(ChallengeResponse {
            challenge,
            expires_in: iot_config.challenge_ttl_seconds,
            platform: Platform::IoT,
        }),
    )
        .into_response())
}