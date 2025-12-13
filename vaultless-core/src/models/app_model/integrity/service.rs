use super::android_offline::verify_android_attestation_offline;
use super::browser::validate_browser_integrity;
use super::dto::{AllowedPlatforms, IntegrityConfig};
use super::ios::verify_ios_attestation;
use super::iot::verify_iot_certificate;
use super::types::*;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde_json::Value as jsonValue;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

const DEFAULT_MAX_FAILED_ATTEMPTS: u32 = 5;
// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Check if a platform is allowed based on the allowed_platforms configuration
pub fn is_platform_allowed(
    platform: Platform,
    allowed_platforms: Option<&AllowedPlatforms>,
) -> bool {
    match (platform, allowed_platforms) {
        (Platform::Android, Some(platforms)) => platforms.android.unwrap_or(true),
        (Platform::IOS, Some(platforms)) => platforms.ios.unwrap_or(true),
        (Platform::IoT, Some(platforms)) => platforms.iot.unwrap_or(true),
        (Platform::Browser, Some(platforms)) => platforms.browser.unwrap_or(true),
        (_, None) => true, // If no allowed_platforms defined, all platforms are allowed
        _ => true,         // Default to allowing platform if not specified in config
    }
}

// =============================================================================
// INTEGRITY SERVICE
// =============================================================================

/// Main integrity service - orchestrates all platform verifications
pub struct IntegrityService {
    redis_pool: Arc<RedisPool>,
    postgres_pool: Arc<PgPool>,
}

impl IntegrityService {
    pub fn new(redis_pool: Arc<RedisPool>, postgres_pool: Arc<PgPool>) -> Self {
        Self {
            redis_pool,
            postgres_pool,
        }
    }

    /// Verify integrity for any platform using strong types
    pub async fn verify_integrity(
        &self,
        request: &AttestationRequest,
        challenge: Option<&str>,
        config: &IntegrityConfig,
        application_id: Uuid,
        client_id: Option<Uuid>,
        ip_address: Option<std::net::IpAddr>,
    ) -> Result<(Platform, AttestationResult)> {
        // 0. Validate that at least one identity parameter is provided
        if client_id.is_none() && ip_address.is_none() {
            return Err(VaultlessError::Validation(
                "Either client_id or ip_address must be provided".to_string(),
            ));
        }

        // 1. Validate request format
        request
            .validate()
            .map_err(|e| VaultlessError::Validation(format!("Invalid integrity request: {}", e)))?;

        // Identity key selection: Use client_id if present, otherwise use IP address
        // Both validated to exist at least one (see validation at start of function)
        let identity_key = match (client_id, ip_address) {
            (Some(id), _) => id.to_string(),
            (None, Some(ip)) => ip.to_string(),
            _ => {
                // This should never happen due to early validation check
                return Err(VaultlessError::Internal(
                    "Invalid identity state: both client_id and ip_address are None".into(),
                ));
            }
        };

        let max_failed_attempts = config
            .rate_limits
            .as_ref()
            .and_then(|limits| limits.max_failed_attempts_before_lockout)
            .unwrap_or(DEFAULT_MAX_FAILED_ATTEMPTS);

        // 1.5 Pre-check: ensure client isn't already locked out due to repeated failures
        // (fast-fail before doing expensive integrity verification)
        check_client_lockout(
            &self.redis_pool,
            &application_id,
            &identity_key,
            max_failed_attempts,
        )
        .await?;

        // 3. Determine the platform from the request
        let platform = match &request.platform_data {
            PlatformAttestationData::Android(_) => Platform::Android,
            PlatformAttestationData::IOS(_) => Platform::IOS,
            PlatformAttestationData::IoT(_) => Platform::IoT,
            PlatformAttestationData::Browser(_) => Platform::Browser,
        };

        // 4. Check if the platform is allowed (early rejection before expensive operations)
        if !is_platform_allowed(platform, config.allowed_platforms.as_ref()) {
            let platform_name = match platform {
                Platform::Android => "Android",
                Platform::IOS => "iOS",
                Platform::IoT => "IoT",
                Platform::Browser => "Browser",
            };
            return Err(VaultlessError::IntegrityCheckFailed(
                format!("{} integrity verification is disabled", platform_name).into(),
            ));
        }

        // 5. Check if unauthenticated mode is enabled
        let allow_unauthenticated = config.allow_unauthenticated.unwrap_or(false);

        // 6. Handle verification based on unauthenticated mode
        if allow_unauthenticated {
            // When allow_unauthenticated is true, only check allowed platforms
            // All platform-specific integrity checks are bypassed
            return Ok((
                platform,
                AttestationResult {
                    is_valid: true,
                    device_trusted: true,
                    trust_score_percent: 100, // Max trust score in unauthenticated mode
                    verdict: Some("Unauthenticated mode enabled".to_string()),
                    error: None,
                    warnings: None,
                    verified_at: Utc::now(),
                    extra: jsonValue::Null,
                },
            ));
        } else {
            // When allow_unauthenticated is false (normal mode), perform full verification
            let (platform, attestation, trust_score): (Platform, AttestationResult, u8) =
                match &request.platform_data {
                    PlatformAttestationData::Android(android_data) => {
                        let android_cfg = config.android.clone().unwrap_or_default();
                        let result = verify_android_attestation_offline(
                            &android_data.attestation_token,
                            &android_cfg,
                        )
                        .await?;

                        (
                            Platform::Android,
                            result,
                            android_cfg.calculate_trust_score(),
                        )
                    }

                    PlatformAttestationData::IOS(ios_data) => {
                        let ios_cfg = config.ios.clone().unwrap_or_default();
                        let result = verify_ios_attestation(
                            &ios_data.attestation_token,
                            &ios_data.ios_version,
                            ios_data.device_model.as_deref(),
                            &ios_cfg,
                        )
                        .await?;

                        (Platform::IOS, result, ios_cfg.calculate_trust_score())
                    }

                    PlatformAttestationData::IoT(iot_data) => {
                        let iot_cfg = config.iot.clone().unwrap_or_default();
                        let result = verify_iot_certificate(
                            Some(&iot_data.device_cn),
                            &iot_data.device_certificate,
                            Some(&iot_data.device_signature),
                            challenge,
                            application_id,
                            &iot_cfg,
                            self.postgres_pool.clone(),
                        )
                        .await?;

                        (Platform::IoT, result, iot_cfg.calculate_trust_score())
                    }

                    PlatformAttestationData::Browser(browser_data) => {
                        // Browser doesn't support hardware attestation, but we can still perform
                        // other validations using the browser_data fields
                        let browser_cfg = config.browser.clone().unwrap_or_default();

                        // Extract IP address from identity_key (this needs to be passed differently)
                        let ip_address = identity_key.parse::<std::net::IpAddr>().unwrap_or_else(|_| {
                            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                        });

                        // Perform browser integrity validation with full checks including:
                        // - Origin validation
                        // - Referer validation
                        // - Rate limiting
                        // - Client-origin binding (if client_id is present)
                        let result = validate_browser_integrity(
                            &self.redis_pool,
                            browser_data,
                            &identity_key,
                            client_id, // Pass the client_id for conditional binding checks
                            ip_address,
                            &browser_cfg,
                        )
                        .await?;

                        (
                            Platform::Browser,
                            result.clone(),
                            result.trust_score_percent, // Use the trust score from the result
                        )
                    }
                };

            // Post-verification: track failures or enforce attestation rate limit
            if attestation.trust_score_percent < trust_score || !attestation.device_trusted {
                track_failed_integrity(
                    &self.redis_pool,
                    &application_id,
                    &identity_key,
                    max_failed_attempts,
                )
                .await?;

                tracing::warn!(
                    identity_key = %identity_key,
                    "Integrity verification failed: trust score {} below required {} or device not trusted",
                    attestation.trust_score_percent,
                    trust_score
                );

                return Err(VaultlessError::Unauthorized(
                    "Integrity verification failed: device not trusted or insufficient trust score"
                        .into(),
                ));
            } else {
                check_integrity_rate_limit(
                    &self.redis_pool,
                    &application_id,
                    &identity_key,
                    platform.as_str(),
                    config
                        .rate_limits
                        .as_ref()
                        .and_then(|rl| rl.max_attestations_per_user_per_hour)
                        .unwrap_or(100),
                )
                .await?;
            }

            Ok((platform, attestation))
        }
    }

    /// Generate universal integrity challenge (used by all platforms)
    pub async fn generate_challenge(&self, ttl_seconds: u64) -> Result<String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|e| VaultlessError::Internal(format!("Random generation failed: {}", e)))?;

        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as BASE64};
        let challenge = BASE64.encode(bytes);

        // Store in Redis with TTL
        let key = cache_key!("integrity_challenge", &challenge);
        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

        let _: () = conn
            .set_ex::<_, _, ()>(&key, "1", ttl_seconds)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis SETEX failed: {}", e)))?;

        Ok(challenge)
    }
}

// =============================================================================
// RATE LIMITING / LOCKOUT
// =============================================================================

const RATE_LIMIT_WINDOW_SECONDS: i64 = 3600;

/// Check rate limit for integrity verification attempts (per app, per identity)
pub async fn check_integrity_rate_limit(
    redis_pool: &RedisPool,
    application_id: &Uuid,
    identity_key: &str,
    platform: &str,
    max_attempts_per_hour: u32,
) -> Result<()> {
    let key = cache_key!(
        "rate_limit",
        "integrity",
        application_id,
        platform,
        identity_key
    );

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    // increment and set TTL on first hit
    let count: u32 = conn
        .incr(&key, 1)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis INCR failed: {}", e)))?;

    if count == 1 {
        let _: () = conn
            .expire(&key, RATE_LIMIT_WINDOW_SECONDS)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    if count > max_attempts_per_hour {
        return Err(VaultlessError::RateLimitExceeded(format!(
            "Too many {} integrity verification attempts. Try again later.",
            platform
        )));
    }

    Ok(())
}

/// Track failed integrity verification attempt (per app, per identity)
pub async fn track_failed_integrity(
    redis_pool: &RedisPool,
    application_id: &Uuid,
    identity_key: &str,
    max_failures: u32,
) -> Result<()> {
    let key = cache_key!(
        "failed_integrity_verifications",
        application_id,
        identity_key
    );

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
            .expire(&key, RATE_LIMIT_WINDOW_SECONDS)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis EXPIRE failed: {}", e)))?;
    }

    if count >= max_failures {
        return Err(VaultlessError::RateLimitExceeded(
            "Too many failed integrity verifications. Account temporarily locked.".into(),
        ));
    }

    Ok(())
}

/// Pre-check whether client is currently locked out due to failed integrity verifications.
/// This avoids doing expensive integrity verification if the client is already locked.
pub async fn check_client_lockout(
    redis_pool: &RedisPool,
    application_id: &Uuid,
    identity_key: &str,
    max_failures: u32,
) -> Result<()> {
    let key = cache_key!(
        "failed_integrity_verifications",
        application_id,
        identity_key
    );

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

    // Read current failure count (0 if missing)
    let count: u32 = conn
        .get(&key)
        .await
        .map_err(|e| {
            // If the GET fails for some reason, avoid silently allowing attacks; surface an internal error
            VaultlessError::Internal(format!("Redis GET failed: {}", e))
        })
        .unwrap_or(0);

    if count >= max_failures {
        return Err(VaultlessError::RateLimitExceeded(
            "Too many failed integrity verifications. Account temporarily locked.".into(),
        ));
    }

    Ok(())
}

/*
Example request shape:

{
  "challenge": "base64-challenge-from-server",
  "challenge_signature": "optional-signature-if-needed",
  "platform_data": {
    "android": {
      "attestation_token": "eyJhbGciOiJSUzI1NiIsImtpZCI6Ik..."
    }
  }
}
*/
