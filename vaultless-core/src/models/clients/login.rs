use super::client_integrity_handler::AttestationRecord;
use super::dto::*;
use crate::models::app_model::integrity::AttestationRequest;
use crate::models::app_model::integrity::Platform;
use crate::models::app_model::integrity::types::PlatformIntegrityData;
use crate::models::session::HybridSessionVerifier;
use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::{
        app_model::{dto::ApplicationKeyView, integrity::IntegrityService},
        session::paseto_session::{self, SessionData, SessionVerifier, verify_session_token},
    },
};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

const SESSION_DURATION_HOURS: u64 = 24 * 30; // 30 days

enum AttestationResult {
    NotRequired(u8),
    Reattested { _previous_score: u8, new_score: u8 },
}

/// Internal struct to hold the results of the common authentication steps
struct CommonAuthResult {
    client: Client,
    device_trust_score: u8,
    was_reattested: bool,
    platform: Platform,
}

impl Client {
    /// Shared authentication logic for both login methods
    async fn authenticate_common<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        app_resolved: Arc<ApplicationKeyView>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: LoginClientRequest,
        platform: Platform,
    ) -> Result<CommonAuthResult>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Step 1: Verify challenge
        Self::verify_and_consume_challenge(&redis, &input.challenge).await?;

        // Step 2: Find and validate client
        let mut client = Self::find_active_client(exec.clone(), &input).await?;

        // Step 3: Verify challenge signature
        Self::verify_challenge_signature(&client, &input)?;

        // Step 4: Handle re-attestation if required
        let attestation_result = Self::handle_reattestation(
            exec,
            &app_resolved,
            integrity_service,
            &mut client,
            &input.platform_data,
            &input.challenge,
        )
        .await?;

        let (device_trust_score, was_reattested) = match attestation_result {
            AttestationResult::NotRequired(score) => (score, false),
            AttestationResult::Reattested { new_score, .. } => (new_score, true),
        };

        Ok(CommonAuthResult {
            client,
            device_trust_score,
            was_reattested,
            platform,
        })
    }

    /// Authenticate client by hashed identifier with optional re-attestation
    pub async fn login<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        session_verifier: Arc<SessionVerifier>,
        app_resolved: Arc<ApplicationKeyView>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: LoginClientRequest,
    ) -> Result<LoginClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Auto-detect platform from the platform data
        let detected_platform = input.platform_data.platform();

        let auth_result = Self::authenticate_common(
            exec.clone(),
            redis,
            app_resolved.clone(),
            integrity_service,
            input,
            detected_platform,
        )
        .await?;

        // Create session and update client
        let response = Self::create_session_and_update(
            exec,
            session_verifier,
            app_resolved,
            auth_result.client,
            auth_result.platform,
            auth_result.device_trust_score,
            auth_result.was_reattested,
        )
        .await?;

        tracing::info!(
            client_id = %response.client_id,
            platform = %auth_result.platform.as_str(),
            was_reattested = %auth_result.was_reattested,
            "Client authenticated successfully",
        );

        Ok(response)
    }

    /// Verify and consume the authentication challenge from Redis
    pub async fn verify_and_consume_challenge(
        redis: &Arc<RedisPool>,
        challenge: &str,
    ) -> Result<()> {
        let challenge_hash = crypto::hash_content(challenge.as_bytes());
        let cache_key = cache_auth_challenge_key(&challenge_hash);

        let mut conn = redis.get().await?;
        let challenge_exists: Option<i32> = conn.get_del(&cache_key).await?;

        if challenge_exists.is_none() {
            tracing::warn!(
                cache_key = %cache_key,
                "Authentication failed: invalid or expired challenge"
            );
            return Err(VaultlessError::Unauthorized(
                "Invalid or expired challenge".into(),
            ));
        }

        Ok(())
    }

    /// Find an active client by one of the provided identifiers
    async fn find_active_client<'c, E>(exec: E, input: &LoginClientRequest) -> Result<Client>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let client = if let Some(ref hash) = input.client_identifier_hash {
            sqlx::query_as::<_, Client>(
                "SELECT * FROM clients WHERE client_identifier_hash = $1 AND is_active = TRUE",
            )
            .bind(hash)
            .fetch_optional(exec)
            .await?
        } else if let Some(ref idf) = input.identifier {
            sqlx::query_as::<_, Client>(
                "SELECT * FROM clients WHERE identifier = $1 AND is_active = TRUE",
            )
            .bind(idf)
            .fetch_optional(exec)
            .await?
        } else if let Some(ref sk) = input.signing_key {
            sqlx::query_as::<_, Client>(
                "SELECT * FROM clients WHERE signing_key = $1 AND is_active = TRUE",
            )
            .bind(sk)
            .fetch_optional(exec)
            .await?
        } else {
            return Err(VaultlessError::Validation(
                "Provide at least one of client_identifier_hash, identifier, or signing_key".into(),
            ));
        };

        let client =
            client.ok_or_else(|| VaultlessError::NotFound("Client not found".to_string()))?;

        if !client.is_active {
            return Err(VaultlessError::Unauthorized(
                "Client is deactivated".to_string(),
            ));
        }

        Ok(client)
    }

    /// Handle re-attestation if required by platform policy
    async fn handle_reattestation<'c, E>(
        exec: E,
        app_resolved: &Arc<ApplicationKeyView>,
        integrity_service: Option<Arc<IntegrityService>>,
        client: &mut Client,
        input: &PlatformIntegrityData,
        challenge: &str,
    ) -> Result<AttestationResult>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Auto-detect platform from the data
        let platform = input.platform();

        let integrity_handler = app_resolved.integrity()?;
        let (trust_score, max_age) = integrity_handler.get_trust_score_and_reattestation(platform);
        let platform_version = integrity_handler.get_platform_config_version().get(platform);

        let client_attestation = client.integrity()?;
        let current_score = client_attestation
            .get_platform_trust_score(platform)
            .unwrap_or(0);

        let requires_reattestation =
            client_attestation.platform_requires_reattestation(platform, trust_score, max_age, platform_version);

        if !requires_reattestation {
            return Ok(AttestationResult::NotRequired(current_score));
        }

        let integrity_svc = integrity_service
            .ok_or_else(|| VaultlessError::Internal("Integrity service not configured".into()))?;

        let (platform_attested, attestation_result) = integrity_svc
            .verify_integrity(
                &AttestationRequest {
                    platform_data: input.clone(),
                },
                Some(challenge), // Use the authentication challenge
                &integrity_handler.config,
                app_resolved.app_id,
                Some(client.id), // Use the existing client id
                None,            // No IP address provided in login
            )
            .await?;

        if platform_attested != platform {
            tracing::warn!(
                client_id = %client.id,
                expected_platform = ?platform,
                actual_platform = ?platform_attested,
                "Platform mismatch during attestation"
            );
            return Err(VaultlessError::Unauthorized(
                "Platform mismatch during attestation".into(),
            ));
        }

        let record: AttestationRecord = attestation_result.into_record(platform_version);
        let new_score = record.trust_score_percent;

        sqlx::query(
            r#"
            UPDATE clients
            SET metadata = jsonb_set(
                COALESCE(metadata, '{}'),
                $1,
                $2,
                true
            )
            WHERE id = $3
            "#,
        )
        .bind(format!("{{{}}}", platform.as_str()))
        .bind(serde_json::to_value(&record)?)
        .bind(client.id)
        .execute(exec)
        .await?;

        Ok(AttestationResult::Reattested {
            _previous_score: current_score,
            new_score,
        })
    }

    /// Verify the challenge signature
    fn verify_challenge_signature(client: &Client, input: &LoginClientRequest) -> Result<()> {
        if !client.verify_signature(&input.challenge, &input.challenge_signature)? {
            return Err(VaultlessError::Unauthorized(
                "Invalid challenge signature".into(),
            ));
        }
        Ok(())
    }

    /// Create session token and update client record in a single transaction
    async fn create_session_and_update<'c, E>(
        exec: E,
        session_verifier: Arc<SessionVerifier>,
        app_resolved: Arc<ApplicationKeyView>,
        client: Client,
        platform: Platform,
        device_trust_score: u8,
        was_reattested: bool,
    ) -> Result<LoginClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let ttl_seconds = SESSION_DURATION_HOURS * 3600;
        let integrity_handler = app_resolved.integrity()?;

        // Retrieve the key manager from the verifier
        let key_manager = session_verifier.key_manager();

        let session_data = SessionData {
            client_id: client.id,
            application_id: client.application_id,
            platform: platform.as_str().to_string(),
            device_trust_score,
            platform_config_version: integrity_handler.platform_config_version.get(platform),
            app_tier: Some(app_resolved.sub_tier.to_string()),
            application_secret_api_key_id: Some(app_resolved.sk_id),
            pubkey: None,
        };

        // Use key_manager.current() for encryption
        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        // Use key_manager for immediate verification (to extract JTI)
        let (_, new_jti) = verify_session_token(&key_manager, &session_token)?;
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // Revoke old session using SessionVerifier
        if let Some(old_jti) = &client.last_jti {
            let verifier = session_verifier.clone();
            let old_jti_clone = old_jti.clone();
            let client_id = client.id;

            tokio::spawn(async move {
                match verifier.revoke_session(&old_jti_clone, ttl_seconds).await {
                    Ok(_) => {
                        tracing::debug!(
                            client_id = %client_id,
                            old_jti = %old_jti_clone,
                            "Successfully revoked previous session"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            client_id = %client_id,
                            old_jti = %old_jti_clone,
                            error = ?e,
                            "Failed to revoke previous session - manual cleanup may be required"
                        );
                    }
                }
            });
        }

        // Update client with new session info
        sqlx::query(
            r#"
            UPDATE clients
            SET last_jti = $1,
                last_seen_at = $2
            WHERE id = $3
            "#,
        )
        .bind(&new_jti)
        .bind(expires_at)
        .bind(client.id)
        .execute(exec)
        .await?;

        Ok(LoginClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
            is_new_session: true,
            was_reattested,
        })
    }

    pub async fn login_hybrid<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        hybrid_verifier: Arc<HybridSessionVerifier>,
        app_resolved: Arc<ApplicationKeyView>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: LoginClientRequest,
    ) -> Result<LoginClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Auto-detect platform from the platform data
        let detected_platform = input.platform_data.platform();

        let auth_result = Self::authenticate_common(
            exec.clone(),
            redis,
            app_resolved.clone(),
            integrity_service,
            input,
            detected_platform,
        )
        .await?;

        // Create session and update client (using hybrid verifier)
        let response = Self::create_session_and_update_hybrid(
            exec,
            hybrid_verifier,
            app_resolved,
            auth_result.client,
            auth_result.platform,
            auth_result.device_trust_score,
            auth_result.was_reattested,
        )
        .await?;

        tracing::info!(
            client_id = %response.client_id,
            platform = %auth_result.platform.as_str(),
            was_reattested = %auth_result.was_reattested,
            "Client authenticated successfully (Hybrid)",
        );

        Ok(response)
    }

    /// Create session token and update client record in a single transaction (Hybrid Verifier)
    async fn create_session_and_update_hybrid<'c, E>(
        exec: E,
        hybrid_verifier: Arc<HybridSessionVerifier>,
        app_resolved: Arc<ApplicationKeyView>,
        client: Client,
        platform: Platform,
        device_trust_score: u8,
        was_reattested: bool,
    ) -> Result<LoginClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let ttl_seconds = SESSION_DURATION_HOURS * 3600;
        let integrity_handler = app_resolved.integrity()?;

        // Use the getter on the hybrid verifier to access the key manager
        let key_manager_arc = hybrid_verifier.key_manager();
        let key_manager = key_manager_arc.as_ref(); // Get reference to SessionKeyManager

        let session_data = SessionData {
            client_id: client.id,
            application_id: client.application_id,
            platform: platform.as_str().to_string(),
            device_trust_score,
            platform_config_version: integrity_handler.platform_config_version.get(platform),
            app_tier: Some(app_resolved.sub_tier.to_string()),
            application_secret_api_key_id: Some(app_resolved.sk_id),
            pubkey: None,
        };

        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        // Need the key_manager for JTI extraction
        let (_, new_jti) = verify_session_token(key_manager, &session_token)?;
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // Revoke old session using HybridVerifier
        if let Some(old_jti) = &client.last_jti {
            let verifier = hybrid_verifier.clone();
            let old_jti_clone = old_jti.clone();
            let client_id = client.id;

            tokio::spawn(async move {
                // Use the HybridVerifier's revoke method
                match verifier.revoke_session(&old_jti_clone, ttl_seconds).await {
                    Ok(_) => {
                        tracing::debug!(
                            client_id = %client_id,
                            old_jti = %old_jti_clone,
                            "Successfully revoked previous session (Hybrid)"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            client_id = %client_id,
                            old_jti = %old_jti_clone,
                            error = ?e,
                            "Failed to revoke previous session (Hybrid) - manual cleanup may be required"
                        );
                    }
                }
            });
        }

        // Update client with new session info - single DB roundtrip
        sqlx::query(
            r#"
            UPDATE clients
            SET last_jti = $1,
                last_seen_at = $2
            WHERE id = $3
            "#,
        )
        .bind(&new_jti)
        .bind(expires_at)
        .bind(client.id)
        .execute(exec)
        .await?;

        Ok(LoginClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
            is_new_session: true,
            was_reattested,
        })
    }
}
