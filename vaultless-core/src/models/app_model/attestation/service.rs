use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use std::sync::Arc;
use validator::Validate;

use super::config::*;
use super::types::*;
use super::android::verify_android_attestation;
use super::ios::{verify_ios_attestation, generate_ios_challenge};
use super::iot::{verify_iot_certificate, generate_iot_challenge, IoTAttestationRequest};

/// Main attestation service - orchestrates all platform verifications
pub struct AttestationService {
    redis_pool: Option<Arc<RedisPool>>,
}

impl AttestationService {
    pub fn new(redis_pool: Option<Arc<RedisPool>>) -> Self {
        Self { redis_pool }
    }

    /// Verify attestation for any platform
    pub async fn verify_attestation(
        &self,
        request: &AttestationRequest,
        integrity_config: &serde_json::Value,
    ) -> Result<AttestationResult> {
        // Validate request
        request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid attestation request: {}", e))
        })?;

        // Route to platform-specific verification
        match request.platform {
            Platform::Android => self.verify_android(request, integrity_config).await,
            Platform::IOS => self.verify_ios(request, integrity_config).await,
            Platform::IoT => self.verify_iot(request, integrity_config).await,
            Platform::Web => Err(VaultlessError::Validation(
                "Web platform does not support mobile attestation".into(),
            )),
        }
    }

    /// Verify Android Play Integrity attestation
    async fn verify_android(
        &self,
        request: &AttestationRequest,
        integrity_config: &serde_json::Value,
    ) -> Result<AttestationResult> {
        let config = extract_android_config(integrity_config)?;

        // Validate bundle ID
        validate_bundle_id(&request.bundle_id, &config.base.allowed_bundle_ids)?;

        // Validate version
        validate_version(
            request.app_version.as_deref(),
            config.base.min_version_code,
        )?;

        // Extract nonce (required for Android)
        let nonce = request.challenge.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Nonce required for Android attestation".into())
        })?;

        // Call Android verification
        verify_android_attestation(
            &request.attestation_token,
            &request.bundle_id,
            &config.certificate_sha256,
            nonce,
            &config.google_cloud_project,
            &config.google_api_key,
            config.max_token_age_seconds,
            config.reject_unrecognized_version,
            config.base.reject_untrusted_device,
        )
        .await
    }

    /// Verify iOS App Attest attestation
    async fn verify_ios(
        &self,
        request: &AttestationRequest,
        integrity_config: &serde_json::Value,
    ) -> Result<AttestationResult> {
        let config = extract_ios_config(integrity_config)?;

        // Validate bundle ID
        validate_bundle_id(&request.bundle_id, &config.base.allowed_bundle_ids)?;

        // Validate version
        validate_version(
            request.app_version.as_deref(),
            config.base.min_version_code,
        )?;

        // Extract challenge (required for iOS)
        let challenge = request.challenge.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Challenge required for iOS attestation".into())
        })?;

        // Call iOS verification
        verify_ios_attestation(
            &request.attestation_token,
            &request.bundle_id,
            &config.apple_team_id,
            challenge,
            &config.allowed_certificate_hashes,
            config.base.reject_untrusted_device,
            self.redis_pool.clone(),
        )
        .await
    }

    /// Verify IoT device certificate attestation
    async fn verify_iot(
        &self,
        request: &AttestationRequest,
        integrity_config: &serde_json::Value,
    ) -> Result<AttestationResult> {
        let config = extract_iot_config(integrity_config)?;

        // Validate device ID (using bundle_id field for IoT)
        validate_bundle_id(&request.bundle_id, &config.base.allowed_bundle_ids)?;

        // Validate firmware version
        validate_version(
            request.app_version.as_deref(),
            config.base.min_version_code,
        )?;

        // Parse IoT-specific attestation token (JSON structure)
        let iot_request: IoTAttestationRequest =
            serde_json::from_str(&request.attestation_token).map_err(|e| {
                VaultlessError::IntegrityCheckFailed(format!("Invalid IoT attestation: {}", e))
            })?;

        iot_request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid IoT attestation request: {}", e))
        })?;

        // Verify device_id matches request
        if iot_request.device_id != request.device_id {
            return Err(VaultlessError::Validation(
                "Device ID mismatch between request and attestation token".into(),
            ));
        }

        // Call IoT verification
        verify_iot_certificate(
            &iot_request.device_certificate,
            &iot_request.challenge_signature,
            &iot_request.challenge,
            &iot_request.device_id,
            &config.allowed_certificate_authorities,
            config.require_cn_match,
            self.redis_pool.clone(),
        )
        .await
    }

    /// Generate challenge for iOS attestation
    pub async fn generate_ios_challenge(
        &self,
        integrity_config: &serde_json::Value,
    ) -> Result<String> {
        let config = extract_ios_config(integrity_config)?;

        let redis_pool = self.redis_pool.as_ref().ok_or_else(|| {
            VaultlessError::Internal("Redis pool required for challenge generation".into())
        })?;

        generate_ios_challenge(redis_pool, config.challenge_ttl_seconds).await
    }

    /// Generate challenge for IoT attestation
    pub async fn generate_iot_challenge(
        &self,
        integrity_config: &serde_json::Value,
    ) -> Result<String> {
        let config = extract_iot_config(integrity_config)?;

        let redis_pool = self.redis_pool.as_ref().ok_or_else(|| {
            VaultlessError::Internal("Redis pool required for challenge generation".into())
        })?;

        generate_iot_challenge(redis_pool, config.challenge_ttl_seconds).await
    }
}

// =============================================================================
// RATE LIMITING (optional, can be extracted to separate module)
// =============================================================================

/// Check rate limit for attestation attempts
pub async fn check_attestation_rate_limit(
    redis_pool: &RedisPool,
    client_id: &str,
    platform: Platform,
    max_attempts_per_hour: u32,
) -> Result<()> {
    use redis::AsyncCommands;

    let key = format!("rate_limit:attestation:{}:{}", platform.as_str(), client_id);
    
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    // Increment counter
    let count: u32 = conn
        .incr(&key, 1)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis INCR failed: {}", e)))?;

    // Set expiry on first attempt
    if count == 1 {
        let _: () = conn
            .expire(&key, 3600) // 1 hour
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    // Check limit
    if count > max_attempts_per_hour {
        return Err(VaultlessError::RateLimitExceeded(format!(
            "Too many {} attestation attempts. Try again later.",
            platform
        )));
    }

    Ok(())
}

/// Track failed attestation attempt
pub async fn track_failed_attestation(
    redis_pool: &RedisPool,
    client_id: &str,
    max_failures: u32,
) -> Result<()> {
    use redis::AsyncCommands;

    let key = format!("failed_attestations:{}", client_id);
    
    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    let count: u32 = conn
        .incr(&key, 1)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis INCR failed: {}", e)))?;

    if count == 1 {
        let _: () = conn
            .expire(&key, 3600) // 1 hour lockout
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    if count >= max_failures {
        return Err(VaultlessError::RateLimitExceeded(
            "Too many failed attestation attempts. Account temporarily locked.".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_service_creation() {
        let service = AttestationService::new(None);
        assert!(service.redis_pool.is_none());
    }

    #[test]
    fn test_invalid_platform_rejection() {
        let request = AttestationRequest {
            platform: Platform::Web,
            bundle_id: "com.example.app".to_string(),
            device_id: "device-123".to_string(),
            attestation_token: "token".to_string(),
            challenge: None,
            app_version: None,
            device_info: None,
        };

        let config = json!({});
        let service = AttestationService::new(None);

        let result = tokio_test::block_on(service.verify_attestation(&request, &config));
        assert!(result.is_err());
    }
}