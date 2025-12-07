use super::android_offline::verify_android_attestation_offline;
use super::dto::IntegrityConfig;
use super::ios::verify_ios_attestation;
use super::iot::verify_iot_certificate;
use super::types::*;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

// =============================================================================
// ATTESTATION SERVICE
// =============================================================================

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
        client_id: Uuid,
    ) -> Result<AttestationResult> {
        // 1. Validate request format
        request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid attestation request: {}", e))
        })?;

        // 1.5 Pre-check: ensure client isn't already locked out due to repeated failures
        // (fast-fail before doing expensive attestation work)
        check_client_lockout(
            &self.redis_pool,
            &application_id,
            &client_id,
            config.rate_limits.max_failed_attempts_before_lockout,
        )
        .await?;

        // 2. Verify challenge exists and is valid (common for all platforms)
        self.verify_and_consume_challenge(&request.challenge)
            .await?;

        // 3. Platform-specific verification (match returns AttestationResult)
        let attestation: AttestationResult = match &request.platform_data {
            PlatformAttestationData::Android(android_data) => {
                verify_android_attestation_offline(&android_data.attestation_token, &config.android)
                    .await?
            }

            PlatformAttestationData::IOS(ios_data) => {
                verify_ios_attestation(
                    &ios_data.attestation_token,
                    &ios_data.ios_version,
                    &config.ios,
                )
                .await?
            }

            PlatformAttestationData::IoT(iot_data) => {
                verify_iot_certificate(
                    Some(&iot_data.device_cn),
                    &iot_data.device_certificate,
                    &iot_data.device_signature,
                    &request.challenge,
                    application_id,
                    &config.iot,
                    self.postgres_pool.clone(),
                )
                .await?
            }

            PlatformAttestationData::Browser(_) => {
                return Err(VaultlessError::Validation(
                    "Browser platform does not support hardware attestation".into(),
                ));
            }
        };

        // Post-verification: track failures or enforce attestation rate limit
        if !attestation.is_valid {
            track_failed_attestation(
                &self.redis_pool,
                &application_id,
                &client_id,
                config.rate_limits.max_failed_attempts_before_lockout,
            )
            .await?;
        } else {
            check_attestation_rate_limit(
                &self.redis_pool,
                &application_id,
                &client_id,
                attestation.platform.as_str(),
                config.rate_limits.max_attestations_per_user_per_hour,
            )
            .await?;
        }

        Ok(attestation)
    }

    // =========================================================================
    // CHALLENGE MANAGEMENT
    // =========================================================================

    /// Verify challenge exists and consume it (one-time use)
    ///
    /// Uses `DEL` and checks the deleted count so the operation is effectively
    /// atomic from the perspective of "consume-if-present".
    async fn verify_and_consume_challenge(&self, challenge: &str) -> Result<()> {
        let key = cache_key!("attestation_challenge", challenge);
        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {}", e)))?;

        // Attempt to delete the key. `del` returns the number of deleted keys.
        let deleted: i64 = conn
            .del(&key)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis DEL failed: {}", e)))?;

        if deleted == 0 {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Challenge expired, invalid, or already used".into(),
            ));
        }

        Ok(())
    }

    /// Generate universal attestation challenge (used by all platforms)
    pub async fn generate_challenge(&self, ttl_seconds: u64) -> Result<String> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|e| VaultlessError::Internal(format!("Random generation failed: {}", e)))?;

        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as BASE64};
        let challenge = BASE64.encode(bytes);

        // Store in Redis with TTL
        let key = cache_key!("attestation_challenge", &challenge);
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

/// Check rate limit for attestation attempts (per app, per client)
pub async fn check_attestation_rate_limit(
    redis_pool: &RedisPool,
    application_id: &Uuid,
    client_id: &Uuid,
    platform: &str,
    max_attempts_per_hour: u32,
) -> Result<()> {
    let key = cache_key!(
        "rate_limit",
        "attestation",
        application_id,
        platform,
        client_id
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
            "Too many {} attestation attempts. Try again later.",
            platform
        )));
    }

    Ok(())
}

/// Track failed attestation attempt (per app, per client)
pub async fn track_failed_attestation(
    redis_pool: &RedisPool,
    application_id: &Uuid,
    client_id: &Uuid,
    max_failures: u32,
) -> Result<()> {
    let key = cache_key!("failed_attestations", application_id, client_id);

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
            "Too many failed attestation attempts. Account temporarily locked.".into(),
        ));
    }

    Ok(())
}

/// Pre-check whether client is currently locked out due to failed attestations.
/// This avoids doing expensive attestation verification if the client is already locked.
pub async fn check_client_lockout(
    redis_pool: &RedisPool,
    application_id: &Uuid,
    client_id: &Uuid,
    max_failures: u32,
) -> Result<()> {
    let key = cache_key!("failed_attestations", application_id, client_id);

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
            "Too many failed attestation attempts. Account temporarily locked.".into(),
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
