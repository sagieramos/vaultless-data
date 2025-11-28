use super::dto::*;
use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::{
        app_model::{
            attestation::{
                AttestationMetadata, AttestationService, check_attestation_rate_limit,
                track_failed_attestation,
            },
            dto::ApplicationKeyView,
        },
        session::paseto_session::{
            self, SessionData, SessionKeyManager, revoke_session, verify_session_token,
        },
    },
};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

const SESSION_DURATION_HOURS: u64 = 24 * 30; // 30 days

impl Client {
    /// Authenticate client by hashed identifier with optional re-attestation
    pub async fn login<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        key_manager: Arc<SessionKeyManager>,
        app_resolved: ApplicationKeyView,
        attestation_service: Option<Arc<AttestationService>>,
        input: AuthenticateClientRequest,
    ) -> Result<AuthenticateClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let mut was_reattested = false;
        let mut device_trusted = false;
        let mut platform_string = "unknown".to_string();

        // --- 1. Atomically check and consume the challenge from Redis ---
        let challenge_hash = crypto::hash_content(input.challenge.as_bytes());
        let cache_key = cache_auth_challenge_key(&challenge_hash);
        let mut conn = redis.get().await?;

        // Use GETDEL: Get the value and delete the key atomically
        let challenge_check: Option<i32> = conn.get_del(&cache_key).await?;

        if challenge_check.is_none() {
            tracing::warn!(
                "Authentication failed: invalid or expired challenge. Key: {}",
                cache_key
            );
            return Err(VaultlessError::Unauthorized(
                "Invalid or expired challenge".into(),
            ));
        }

        // --- 2. Ensure at least one identifier is provided ---
        if input.client_identifier_hash.is_none()
            && input.identifier.is_none()
            && input.public_key.is_none()
        {
            return Err(VaultlessError::Validation(
                "Provide at least one of client_identifier_hash, identifier, or public_key".into(),
            ));
        }

        // --- 3. Attempt to find the client ---
        let mut client = if let Some(ref hash) = input.client_identifier_hash {
            sqlx::query_as::<_, Client>(
                r#"SELECT * FROM clients WHERE client_identifier_hash = $1 AND is_active = TRUE"#,
            )
            .bind(hash)
            .fetch_optional(exec.clone())
            .await?
        } else if let Some(ref idf) = input.identifier {
            sqlx::query_as::<_, Client>(
                r#"SELECT * FROM clients WHERE identifier = $1 AND is_active = TRUE"#,
            )
            .bind(idf)
            .fetch_optional(exec.clone())
            .await?
        } else if let Some(ref pk) = input.public_key {
            sqlx::query_as::<_, Client>(
                r#"SELECT * FROM clients WHERE public_key = $1 AND is_active = TRUE"#,
            )
            .bind(pk)
            .fetch_optional(exec.clone())
            .await?
        } else {
            None
        }
        .ok_or_else(|| VaultlessError::NotFound("Client not found".to_string()))?;

        // --- 4. Check if client is active ---
        if !client.is_active {
            return Err(VaultlessError::Unauthorized(
                "Client is deactivated".to_string(),
            ));
        }

        // Load initial platform/trust state from existing metadata
        if let Ok(Some(meta)) = AttestationMetadata::from_metadata(client.metadata.as_ref()) {
            if let Some(p) = meta.platform {
                platform_string = p.as_str().to_string();
            }
            device_trusted = meta.is_device_trusted();
        }

        // --- 5. Check if re-attestation is required ---
        // Note: needs_reattesation logic likely checks the metadata inside Client
        let requires_reattestation = client.needs_reattesation(30);

        if requires_reattestation && input.attestation.is_none() {
            tracing::warn!(
                client_id = %client.id,
                "Re-attestation required but not provided"
            );

            return Err(VaultlessError::Unauthorized(
                "Re-attestation required. Please provide attestation token.".into(),
            ));
        }

        // --- 6. If attestation provided, verify it ---
        if let Some(attestation_request) = input.attestation {
            was_reattested = true;
            platform_string = attestation_request.platform.as_str().to_string();

            tracing::info!(
                client_id = %client.id,
                platform = %attestation_request.platform,
                device_id = %attestation_request.device_id,
                "Verifying platform attestation during authentication"
            );

            // Rate limiting check
            let rate_limit = app_resolved
                .integrity()
                .get_attestation_rate_limit(attestation_request.platform);

            if let Err(e) = check_attestation_rate_limit(
                &redis,
                &attestation_request.device_id,
                attestation_request.platform,
                rate_limit,
            )
            .await
            {
                tracing::warn!(
                    client_id = %client.id,
                    platform = %attestation_request.platform,
                    device_id = %attestation_request.device_id,
                    "Rate limit exceeded during authentication"
                );
                return Err(e);
            }

            // Verify attestation using new service
            let attestation_svc = attestation_service.ok_or_else(|| {
                VaultlessError::Internal("Attestation service not configured".into())
            })?;

            let integrity_config =
                serde_json::to_value(&app_resolved.integrity().get_integrity_config()?)
                    .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

            let attestation_result = attestation_svc
                .verify_attestation(&attestation_request, &integrity_config, app_resolved.app_id)
                .await;

            match attestation_result {
                Ok(result) => {
                    if !result.is_valid {
                        // Track failed attempt
                        let max_failures = app_resolved.integrity().get_max_failed_attempts();
                        let _ = track_failed_attestation(
                            &redis,
                            &attestation_request.device_id,
                            max_failures,
                        )
                        .await;

                        tracing::warn!(
                            client_id = %client.id,
                            platform = %attestation_request.platform,
                            device_id = %attestation_request.device_id,
                            verdict = ?result.verdict,
                            error = ?result.error,
                            "Platform attestation failed during authentication"
                        );

                        return Err(VaultlessError::IntegrityCheckFailed(
                            result
                                .error
                                .unwrap_or_else(|| "Attestation verification failed".to_string()),
                        ));
                    }

                    // Check if untrusted devices should be rejected
                    if !result.device_trusted
                        && app_resolved
                            .integrity()
                            .should_reject_untrusted_device(attestation_request.platform)
                    {
                        tracing::warn!(
                            client_id = %client.id,
                            platform = %attestation_request.platform,
                            device_id = %attestation_request.device_id,
                            "Untrusted device rejected during authentication"
                        );

                        return Err(VaultlessError::IntegrityCheckFailed(
                            "Device did not pass integrity checks".to_string(),
                        ));
                    }

                    // Update local trust state
                    device_trusted = result.device_trusted;

                    // Update client metadata with new attestation
                    let mut attestation_meta =
                        AttestationMetadata::from_metadata(client.metadata.as_ref())?
                            .unwrap_or_default();

                    attestation_meta.update_from_result(result);

                    let updated_metadata =
                        attestation_meta.merge_into_metadata(client.metadata.clone())?;

                    // Persist metadata update
                    sqlx::query(
                        "UPDATE clients SET metadata = $1, is_platform_attested = TRUE WHERE id = $2",
                    )
                    .bind(&updated_metadata)
                    .bind(client.id)
                    .execute(exec.clone())
                    .await?;

                    // Update local client struct to reflect metadata change (for session token generation context)
                    client.metadata = Some(updated_metadata);

                    tracing::info!(
                        client_id = %client.id,
                        platform = %attestation_request.platform,
                        device_id = %attestation_request.device_id,
                        device_trusted = attestation_meta.is_device_trusted(),
                        "Re-attestation successful during authentication"
                    );
                }
                Err(e) => {
                    // Track failed attempt
                    let max_failures = app_resolved.integrity().get_max_failed_attempts();
                    let _ = track_failed_attestation(
                        &redis,
                        &attestation_request.device_id,
                        max_failures,
                    )
                    .await;

                    tracing::error!(
                        client_id = %client.id,
                        platform = %attestation_request.platform,
                        device_id = %attestation_request.device_id,
                        error = %e,
                        "Attestation verification error during authentication"
                    );

                    return Err(VaultlessError::IntegrityCheckFailed(
                        "Attestation verification failed".to_string(),
                    ));
                }
            }
        }

        // --- 7. Verify the signed challenge ---
        if !client.verify_signature(&input.challenge, &input.challenge_signature)? {
            return Err(VaultlessError::Unauthorized(
                "Invalid challenge signature".into(),
            ));
        }

        // --- 8. Generate PASETO Session Token ---
        let ttl_seconds = SESSION_DURATION_HOURS * 3600;

        // Prepare session claims
        let session_data = SessionData {
            client_id: client.id,
            application_id: client.application_id,
            platform: platform_string,
            device_trusted,
            app_tier: None,
            application_secret_api_key_id: Some(app_resolved.sk_id),
            pubkey: None,
        };

        // Generate the token
        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        // IMPORTANT: Parse the token immediately to extract the JTI (Token Identifier).
        // We need this to store in `last_jti` for revocation support.
        let (_, new_jti) = verify_session_token(&key_manager, &session_token)?;

        // --- 9. Handle Session Revocation & DB Update ---

        // If a previous session exists, revoke it in Redis
        if let Some(old_jti) = &client.last_jti {
            // Revoke for the duration of the remaining session window (safeguard default 30 days)
            // We use a background spawn or just await it. Since speed matters, we await but ignore errors?
            // Better to await to ensure security.
            let _ = revoke_session(&redis, old_jti, ttl_seconds).await;
            tracing::debug!(client_id = %client.id, old_jti = %old_jti, "Revoked previous session JTI");
        }

        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // Update client with new JTI and last seen
        sqlx::query(
            r#"
            UPDATE clients
            SET last_jti = $1,
                last_seen_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(&new_jti)
        .bind(client.id)
        .execute(exec)
        .await?;

        tracing::info!(
            client_id = %client.id,
            was_reattested = %was_reattested,
            jti = %new_jti,
            "Client authenticated successfully via PASETO"
        );

        Ok(AuthenticateClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
            is_new_session: true,
            was_reattested,
        })
    }
}
