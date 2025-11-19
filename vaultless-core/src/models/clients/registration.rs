use super::dto::*;
use crate::cache_key;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::{AsyncCommands, Script};
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use validator::Validate;

use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::app_model::{
        attestation::{
            AttestationMetadata, AttestationService, Platform, check_attestation_rate_limit,
            dto::IntegrityConfig, track_failed_attestation,
        },
        dto::Application,
    },
};

impl Client {
    /// Register new client with optional platform attestation
    pub async fn register<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        attestation_service: Option<Arc<AttestationService>>,
        input: RegisterClientRequest,
        publishable_key: String,
        // NEW: Web-specific params
        request_headers: Option<std::collections::HashMap<String, String>>,
        ip_address: Option<String>,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
    {
        // --- 1. Validate input ---
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let pubkey = input
            .public_key
            .ok_or_else(|| VaultlessError::Validation("public_key is required.".into()))?;

        let signature = input
            .signature
            .ok_or_else(|| VaultlessError::Validation("signature is required.".into()))?;

        let payload = input
            .signed_payload
            .ok_or_else(|| VaultlessError::Validation("signed_payload is required.".into()))?;

        // --- 2. Verify signature ---
        crate::crypto::verify_signature(payload.as_bytes(), &signature, &pubkey)
            .map_err(|_| VaultlessError::Validation("Signature verification failed".into()))?;

        // --- 3. Nonce replay protection ---
        if let (Some(nonce), Some(redis_pool)) = (&input.nonce, &redis) {
            let nonce_key = cache_key!("client", "register_nonce", nonce);

            if let Ok(mut conn) = redis_pool.get().await {
                const NONCE_SCRIPT: &str = r#"
                    local key = KEYS[1]
                    local ttl = tonumber(ARGV[1])
                    local ok = redis.call('SET', key, '1', 'NX', 'EX', ttl)
                    return ok and 1 or 0
                "#;

                let script = Script::new(NONCE_SCRIPT);
                let result: redis::RedisResult<i32> = script
                    .key(&nonce_key)
                    .arg(IDENTIFIER_TTL_SECS)
                    .invoke_async(&mut conn)
                    .await;

                match result {
                    Ok(1) => tracing::debug!("Nonce reserved"),
                    Ok(0) => return Err(VaultlessError::Validation("Nonce already used".into())),
                    Err(e) => tracing::warn!("Redis nonce check failed: {}", e),
                    _ => {
                        return Err(VaultlessError::Validation(
                            "Nonce check unexpected result".into(),
                        ));
                    }
                }
            }
        }

        // --- 4. Fetch application ---
        let auth_config =
            Application::fetch_full_auth_by_publishable_key(exec.clone(), &publishable_key)
                .await?
                .ok_or(VaultlessError::NotFound(
                    "Auth configuration not found".to_string(),
                ))?;

        // ============= 5. PLATFORM ATTESTATION (NEW) =============

        let mut is_platform_attested = false;
        let mut merged_metadata = input.metadata.clone();

        if let Some(attestation_request) = input.attestation {
            // Validate attestation request
            attestation_request.validate().map_err(|e| {
                VaultlessError::Validation(format!("Invalid attestation request: {}", e))
            })?;

            tracing::info!(
                platform = %attestation_request.platform,
                bundle_id = %attestation_request.bundle_id,
                device_id = %attestation_request.device_id,
                "Verifying platform attestation during registration"
            );

            // Rate limiting check
            if let Some(redis_pool) = &redis {
                // Extract rate limit from integrity_config
                let integrity_config: IntegrityConfig =
                    serde_json::from_value(auth_config.app_integrity_config.clone())
                        .unwrap_or_default();

                let rate_limit = integrity_config
                    .rate_limits
                    .max_attestations_per_user_per_hour;

                if let Err(e) = check_attestation_rate_limit(
                    redis_pool,
                    &attestation_request.device_id,
                    attestation_request.platform,
                    rate_limit,
                )
                .await
                {
                    tracing::warn!(
                        platform = %attestation_request.platform,
                        device_id = %attestation_request.device_id,
                        "Rate limit exceeded during registration"
                    );
                    return Err(e);
                }
            }

            // Verify attestation using new service
            let attestation_svc = attestation_service.ok_or_else(|| {
                VaultlessError::Internal("Attestation service not configured".into())
            })?;

            let attestation_result = attestation_svc
                .verify_attestation(
                    &attestation_request,
                    &auth_config.app_integrity_config,
                    auth_config.app_id,
                )
                .await;

            match attestation_result {
                Ok(result) => {
                    if !result.is_valid {
                        // Track failed attempt
                        if let Some(redis_pool) = &redis {
                            let integrity_config: IntegrityConfig =
                                serde_json::from_value(auth_config.app_integrity_config.clone())
                                    .unwrap_or_default();

                            let max_failures = integrity_config
                                .rate_limits
                                .max_failed_attempts_before_lockout;
                            let _ = track_failed_attestation(
                                redis_pool,
                                &attestation_request.device_id,
                                max_failures,
                            )
                            .await;
                        }

                        tracing::warn!(
                            platform = %attestation_request.platform,
                            device_id = %attestation_request.device_id,
                            verdict = ?result.verdict,
                            error = ?result.error,
                            "Platform attestation failed during registration"
                        );

                        return Err(VaultlessError::IntegrityCheckFailed(
                            result
                                .error
                                .unwrap_or_else(|| "Attestation verification failed".to_string()),
                        ));
                    }

                    // Check if untrusted devices should be rejected
                    if !result.device_trusted {
                        let integrity_config: IntegrityConfig =
                            serde_json::from_value(auth_config.app_integrity_config.clone())
                                .unwrap_or_default();

                        let should_reject = match attestation_request.platform {
                            Platform::IOS => integrity_config.ios.reject_untrusted_device,
                            Platform::Android => integrity_config.android.reject_untrusted_device,
                            Platform::IoT => true, // IoT always requires trusted devices
                            Platform::Browser => false,
                        };

                        if should_reject {
                            tracing::warn!(
                                platform = %attestation_request.platform,
                                device_id = %attestation_request.device_id,
                                "Untrusted device rejected"
                            );

                            return Err(VaultlessError::IntegrityCheckFailed(
                                "Device did not pass integrity checks".to_string(),
                            ));
                        }
                    }

                    // Create attestation metadata
                    let attestation_meta = AttestationMetadata::from_result(
                        result,
                        attestation_request.device_id.clone(),
                        attestation_request.app_version.clone(),
                        attestation_request.device_info.clone(),
                    );

                    // Merge attestation metadata into client metadata
                    merged_metadata = Some(attestation_meta.merge_into_metadata(merged_metadata)?);
                    is_platform_attested = true;

                    tracing::info!(
                        platform = %attestation_request.platform,
                        bundle_id = %attestation_request.bundle_id,
                        device_id = %attestation_request.device_id,
                        device_trusted = attestation_meta.is_device_trusted(),
                        "Platform attestation successful during registration"
                    );
                }
                Err(e) => {
                    // Track failed attempt
                    if let Some(redis_pool) = &redis {
                        let integrity_config: IntegrityConfig =
                            serde_json::from_value(auth_config.app_integrity_config.clone())
                                .unwrap_or_default();

                        let max_failures = integrity_config
                            .rate_limits
                            .max_failed_attempts_before_lockout;
                        let _ = track_failed_attestation(
                            redis_pool,
                            &attestation_request.device_id,
                            max_failures,
                        )
                        .await;
                    }

                    tracing::error!(
                        platform = %attestation_request.platform,
                        device_id = %attestation_request.device_id,
                        error = %e,
                        "Attestation verification error during registration"
                    );

                    return Err(VaultlessError::IntegrityCheckFailed(
                        "Attestation verification failed".to_string(),
                    ));
                }
            }
        } else {
            // Check if attestation is required
            if let Some(attestation_platform) = input.attestation_platform {
                let integrity_config: IntegrityConfig =
                    serde_json::from_value(auth_config.app_integrity_config.clone())
                        .unwrap_or_default();

                let requires_attestation = match attestation_platform {
                    Platform::IOS => {
                        integrity_config.ios.apple_team_id.is_some()
                            || !integrity_config.ios.allowed_bundle_ids.is_empty()
                    }
                    Platform::Android => integrity_config
                        .android
                        .allowed_certificate_sha256
                        .is_some(),
                    Platform::IoT => {
                        integrity_config.iot.require_device_certificate
                            && !integrity_config
                                .iot
                                .allowed_certificate_authorities
                                .is_empty()
                    }
                    Platform::Browser => false,
                };

                if requires_attestation {
                    tracing::warn!(
                        platform = %attestation_platform,
                        "Attestation required but not provided"
                    );

                    return Err(VaultlessError::IntegrityCheckFailed(format!(
                        "Platform attestation required for {} but not provided",
                        attestation_platform
                    )));
                }
            }
        }

        // ============= END ATTESTATION =============

        // --- 6. Compute identifier hash (if provided) ---
        let client_identifier_hash = input
            .client_identifier
            .as_ref()
            .map(|ci| crypto::hash_content(ci.as_bytes()));

        // --- 7. Generate session token ---
        let token = crypto::generate_secure_token::<32>()?;
        let session_token = BASE64.encode(token);
        let session_token_hash = crypto::hash_content(&token);
        let session_expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS);

        // --- 8. Insert client into DB ---
        let client = sqlx::query_as::<_, Client>(
            r#"
            INSERT INTO clients (
                identifier,
                client_identifier_hash,
                public_key,
                session_token_hash,
                session_expires_at,
                metadata,
                developer_id,
                application_id,
                last_seen_at,
                is_platform_attested
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NOW(), $9)
            RETURNING *
            "#,
        )
        .bind(&input.identifier)
        .bind(&client_identifier_hash)
        .bind(&pubkey)
        .bind(&session_token_hash)
        .bind(session_expires_at)
        .bind(&merged_metadata)
        .bind(auth_config.app_user_id)
        .bind(auth_config.app_id)
        .bind(is_platform_attested)
        .fetch_one(exec)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Client already registered".into())
            }
            _ => VaultlessError::Database(e),
        })?;

        tracing::info!(
            client_id = %client.id,
            application_id = %auth_config.app_id,
            developer_id = %auth_config.app_user_id,
            is_platform_attested = %is_platform_attested,
            "Client registered successfully"
        );

        // --- 9. Cache in Redis (non-critical) ---
        if let Some(pool) = &redis {
            let _ = Self::cache_to_redis(pool, &client).await;
        }

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at: session_expires_at,
        })
    }

    // ============= RE-ATTESTATION METHOD =============

    /// Re-attest an existing client (periodic verification)
    pub async fn re_attest<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        attestation_service: Arc<AttestationService>,
        client_id: uuid::Uuid,
        attestation_request: crate::models::app_model::attestation::AttestationRequest,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Fetch client
        let client =
            sqlx::query_as::<_, Client>("SELECT * FROM clients WHERE id = $1 AND is_active = TRUE")
                .bind(client_id)
                .fetch_optional(exec.clone())
                .await?
                .ok_or_else(|| VaultlessError::NotFound("Client not found".into()))?;

        // 2. Fetch application
        let app = sqlx::query_as::<_, Application>(
            "SELECT * FROM applications WHERE id = $1 AND is_active = TRUE",
        )
        .bind(client.application_id)
        .fetch_optional(exec.clone())
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        // 3. Validate request
        attestation_request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid attestation request: {}", e))
        })?;

        // 4. Rate limiting
        if let Some(redis_pool) = &redis {
            let rate_limit = app.get_attestation_rate_limit(attestation_request.platform);

            check_attestation_rate_limit(
                redis_pool,
                &attestation_request.device_id,
                attestation_request.platform,
                rate_limit,
            )
            .await?;
        }

        // 5. Verify attestation
        let integrity_config = serde_json::to_value(&app.get_integrity_config()?)
            .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

        let attestation_result = attestation_service
            .verify_attestation(
                &attestation_request,
                &integrity_config,
                client.application_id,
            )
            .await?;

        if !attestation_result.is_valid {
            // Track failed attempt
            if let Some(redis_pool) = &redis {
                let max_failures = app.get_max_failed_attempts();
                let _ = track_failed_attestation(
                    redis_pool,
                    &attestation_request.device_id,
                    max_failures,
                )
                .await;
            }

            return Err(VaultlessError::IntegrityCheckFailed(
                attestation_result
                    .error
                    .unwrap_or_else(|| "Re-attestation failed".to_string()),
            ));
        }

        // 6. Check device trust
        if !attestation_result.device_trusted
            && app.should_reject_untrusted_device(attestation_request.platform)
        {
            return Err(VaultlessError::IntegrityCheckFailed(
                "Device did not pass integrity checks".to_string(),
            ));
        }

        // 7. Update metadata
        let mut attestation_meta =
            AttestationMetadata::from_metadata(client.metadata.as_ref())?.unwrap_or_default();

        attestation_meta.update_from_result(attestation_result);

        let updated_metadata = attestation_meta.merge_into_metadata(client.metadata.clone())?;

        // 8. Update database
        sqlx::query(
            r#"
            UPDATE clients
            SET metadata = $1,
                is_platform_attested = TRUE,
                updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(&updated_metadata)
        .bind(client_id)
        .execute(exec.clone())
        .await?;

        // 9. Invalidate Redis cache
        if let Some(redis_pool) = redis {
            if let Ok(mut conn) = redis_pool.get().await {
                let cache_key = cache_key!("client", "id", client_id);
                let _ = conn.del::<_, ()>(&cache_key).await;
            }
        }

        tracing::info!(
            client_id = %client_id,
            platform = %attestation_request.platform,
            "Client re-attestation successful"
        );

        Ok(())
    }
}
