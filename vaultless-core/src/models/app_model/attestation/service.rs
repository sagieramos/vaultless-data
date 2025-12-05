use super::andriod_offline::verify_android_attestation_offline;
use super::config::*;
use super::dto::{AndroidIntegrityConfig, IntegrityConfig, IoTIntegrityConfig, IosIntegrityConfig};
use super::ios::{generate_ios_challenge, verify_ios_attestation};
use super::iot::{IoTAttestationRequest, verify_iot_certificate};
use super::types::*;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

/// Main attestation service - orchestrates all platform verifications
pub struct AttestationService {
    redis_pool: Arc<RedisPool>,
    postgres_pool: Arc<PgPool>,
}
impl AttestationService {
    pub fn new(redis_pool: Arc<RedisPool>, postgres_pool: Arc<PgPool>) -> Self {
        Self {
            redis_pool,
            postgres_pool,
        }
    }

    /// Verify attestation for any platform using strong types
    pub async fn verify_attestation(
        &self,
        request: &AttestationRequest,
        config: &IntegrityConfig, 
        application_id: Uuid,
    ) -> Result<AttestationResult> {
        // Validate request format
        request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid attestation request: {}", e))
        })?;

        // Route to platform-specific verification
        match request.platform {
            Platform::Android => self.verify_android_offline(request, &config.android).await,
            Platform::IOS => self.verify_ios(request, &config.ios).await,
            Platform::IoT => self.verify_iot(request, &config.iot, application_id).await,
            Platform::Browser => Err(VaultlessError::Validation(
                "Browser platform does not support mobile attestation".into(),
            )),
        }
    }

    /// Verify Android Play Integrity attestation
    async fn verify_android_online(
        &self,
        request: &AttestationRequest,
        config: &AndroidIntegrityConfig,
    ) -> Result<AttestationResult> {
        // 1. Validate Bundle ID
        validate_identifier(&request.bundle_id, &config.allowed_bundle_ids, "Bundle ID")?;

        // 2. Validate App Version
        validate_version(request.app_version.as_deref(), config.min_version_code)?;

        // 3. Extract Nonce
        let nonce = request.challenge.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Nonce required for Android attestation".into())
        })?;

        let cloud_project = config.google_cloud_project.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "google_cloud_project is required for online attestation".into(),
            )
        })?;

        let api_key = config.google_api_key.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "google_api_key is required for online attestation".into(),
            )
        })?;

        // 4. Required: expected certificate hash
        let cert_hash = config
            .allowed_certificate_sha256
            .as_deref()
            .ok_or_else(|| {
                VaultlessError::IntegrityCheckFailed(
                    "allowed_certificate_sha256 is required for offline attestation".into(),
                )
            })?;

        // 5. Verify via Google Play API
        verify_android_attestation_online(
            &request.attestation_token,
            &request.bundle_id,
            &cert_hash,
            nonce,
            cloud_project,
            api_key,
            config.max_token_age_seconds,
            config.reject_unrecognized_version,
            config.reject_untrusted_device,
        )
        .await
    }

    /// Verify Android Offline attestation
    async fn verify_android_offline(
        &self,
        request: &AttestationRequest,
        config: &AndroidIntegrityConfig,
    ) -> Result<AttestationResult> {
        // 1. Validate Bundle ID
        validate_identifier(&request.bundle_id, &config.allowed_bundle_ids, "Bundle ID")?;

        // 2. Validate App Version
        validate_version(request.app_version.as_deref(), config.min_version_code)?;

        // 3. Extract Nonce
        let nonce = request.challenge.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Nonce required for Android attestation".into())
        })?;

        // 4. Required: expected certificate hash
        let cert_hash = config
            .allowed_certificate_sha256
            .as_deref()
            .ok_or_else(|| {
                VaultlessError::IntegrityCheckFailed(
                    "allowed_certificate_sha256 is required for offline attestation".into(),
                )
            })?;

        // 5. Verify via Offline method
        verify_android_attestation_offline(
            &request.attestation_token,
            &request.bundle_id,
            &cert_hash,
            nonce,
            config.max_token_age_seconds,
            config.reject_unrecognized_version,
            config.reject_untrusted_device,
        )
        .await
    }

    /// Verify iOS App Attest attestation
    async fn verify_ios(
        &self,
        request: &AttestationRequest,
        config: &IosIntegrityConfig,
    ) -> Result<AttestationResult> {
        // 1. Validate Bundle ID
        validate_identifier(&request.bundle_id, &config.allowed_bundle_ids, "Bundle ID")?;

        // 2. Validate App Version
        validate_version(request.app_version.as_deref(), config.min_version_code)?;

        // 3. Extract Challenge
        let expected_challenge = request.challenge.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed("Challenge required for iOS attestation".into())
        })?;

        // 4. Required: expected Apple Team ID
        let team_id = config.apple_team_id.as_deref().ok_or_else(|| {
            VaultlessError::IntegrityCheckFailed(
                "apple_team_id is required for iOS attestation".into(),
            )
        })?;

        // 5. Verify via Apple App Attest
        verify_ios_attestation(
            &request.attestation_token,
            &request.bundle_id,
            &team_id,
            expected_challenge,
            &config.allowed_certificate_hashes,
            config.reject_untrusted_device,
            Some(self.redis_pool.clone()),
        )
        .await
    }

    /// Verify IoT device certificate attestation
    async fn verify_iot(
        &self,
        request: &AttestationRequest,
        config: &IoTIntegrityConfig,
        application_id: Uuid,
    ) -> Result<AttestationResult> {
        // 1. Validate Device ID (mapped to allowed_device_ids in config)
        validate_identifier(&request.bundle_id, &config.allowed_device_ids, "Device ID")?;

        // 2. Validate Firmware Version
        validate_version(request.app_version.as_deref(), config.min_firmware_version)?;

        // 3. Parse IoT Token
        let iot_request: IoTAttestationRequest = serde_json::from_str(&request.attestation_token)
            .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid IoT attestation JSON: {}", e))
        })?;

        iot_request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid IoT attestation request: {}", e))
        })?;

        // 4. Consistency Check
        if iot_request.device_id != request.device_id {
            return Err(VaultlessError::Validation(
                "Device ID mismatch between request and attestation token".into(),
            ));
        }

        // 5. Verify Certificate Chain
        verify_iot_certificate(
            &iot_request.device_certificate,
            &iot_request.challenge_signature,
            &iot_request.challenge,
            &iot_request.device_id,
            &config.allowed_certificate_authorities,
            config.require_cn_match,
            Arc::clone(&self.redis_pool),   
            Arc::clone(&self.postgres_pool),
            application_id,
        )
        .await
    }

    /// Generate challenge for iOS attestation
    pub async fn generate_ios_challenge(&self, config: &IosIntegrityConfig) -> Result<String> {
        generate_ios_challenge(&*self.redis_pool, config.challenge_ttl_seconds).await
    }

    /// Generate challenge for IoT attestation
    pub async fn generate_iot_challenge(&self, config: &IoTIntegrityConfig) -> Result<String> {
        generate_iot_challenge(&*self.redis_pool, config.challenge_ttl_seconds).await
    }
}

// =============================================================================
// HELPER FUNCTIONS (No longer extracting from JSON)
// =============================================================================

fn validate_identifier(id: &str, allowed_list: &[String], id_type: &str) -> Result<()> {
    if allowed_list.is_empty() {
        return Ok(()); // No restrictions if list is empty
    }
    if !allowed_list.contains(&id.to_string()) {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "{} '{}' is not in the allowed list",
            id_type, id
        )));
    }
    Ok(())
}

fn validate_version(current_version: Option<&str>, min_version: Option<i32>) -> Result<()> {
    // If no minimum version is set, everything is allowed
    let min_v = match min_version {
        Some(v) => v,
        None => return Ok(()),
    };

    // If minimum version is set but current version is missing -> Fail
    let current_v_str = current_version.ok_or_else(|| {
        VaultlessError::IntegrityCheckFailed("Version code is required by configuration".into())
    })?;

    // Parse current version
    let current_v_int: i32 = current_v_str.parse().map_err(|_| {
        VaultlessError::Validation(format!("Invalid version code format: {}", current_v_str))
    })?;

    if current_v_int < min_v {
        return Err(VaultlessError::IntegrityCheckFailed(format!(
            "Version {} is below minimum required version {}",
            current_v_int, min_v
        )));
    }

    Ok(())
}

// =============================================================================
// RATE LIMITING
// =============================================================================

/// Check rate limit for attestation attempts
pub async fn check_attestation_rate_limit(
    redis_pool: &RedisPool,
    client_id: &str,
    platform: Platform,
    max_attempts_per_hour: u32,
) -> Result<()> {
    use redis::AsyncCommands;
    let key = cache_key!("rate_limit", "attestation", platform.as_str(), client_id);
    let mut conn = redis_pool.get().await?;

    let count: u32 = conn.incr(&key, 1).await?;
    if count == 1 {
        let _: () = conn.expire(&key, 3600).await?;
    }

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
    let key = cache_key!("failed_attestations", client_id);
    let mut conn = redis_pool.get().await?;

    let count: u32 = conn.incr(&key, 1).await?;
    if count == 1 {
        let _: () = conn.expire(&key, 3600).await?;
    }

    if count >= max_failures {
        return Err(VaultlessError::RateLimitExceeded(
            "Too many failed attestation attempts. Account temporarily locked.".into(),
        ));
    }
    Ok(())
}
