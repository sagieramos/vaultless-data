use super::dto::*;
use crate::models::app_model::integrity::AttestationRequest;
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
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;

const SESSION_DURATION_HOURS: u64 = 24 * 30; // 30 days

impl Client {
    /// Register new client with platform attestation
    pub async fn sign_up<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        session_verifier: Arc<SessionVerifier>,
        app_resolved: Arc<ApplicationKeyView>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: SignupClientRequest,
        ip_address: std::net::IpAddr,
    ) -> Result<SignupClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Step 1: Verify challenge
        Self::verify_and_consume_challenge(&redis, &input.challenge).await?;

        // Step 2: Verify challenge signature using Ed25519 signing key
        let signing_key = input.signing_key.clone();
        Self::verify_signup_challenge_signature(
            &signing_key,
            &input.challenge,
            &input.challenge_signed,
        )?;

        // Step 3: Platform attestation
        let attestation_result = if let Some(integrity_svc) = integrity_service {
            let detected_platform = input.platform_data.platform();

            let integrity_handler = app_resolved.integrity()?;
            let (attested_platform, attestation_result) = integrity_svc
                .verify_integrity(
                    &AttestationRequest {
                        platform_data: input.platform_data.clone(), // Clone to avoid moving
                    },
                    Some(&input.challenge), // Use the signup challenge
                    &integrity_handler.config,
                    app_resolved.app_id,
                    None, // No client_id available during registration
                    Some(ip_address),
                )
                .await?;

            if attested_platform != detected_platform {
                return Err(VaultlessError::Unauthorized(
                    "Platform mismatch during attestation".into(),
                ));
            }

            Some(attestation_result)
        } else {
            None
        };

        // Step 4: Generate unique client identifier hash if provided
        let client_identifier_hash = input
            .client_identifier
            .as_ref()
            .map(|ci| crypto::hash_content(ci.as_bytes()));

        // Step 5: Create new client in database
        let client_id = Uuid::new_v4();
        let device_trust_score = attestation_result
            .as_ref()
            .map(|ar| ar.trust_score_percent)
            .unwrap_or(0);
        let is_platform_attested = attestation_result.is_some();

        let client = sqlx::query_as::<_, Client>(&format!(
            "INSERT INTO clients ({}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NOW(),$9) RETURNING *",
            MINIMAL_CLIENT_FIELDS
        ))
        .bind(client_id)
        .bind(&input.identifier)
        .bind(&client_identifier_hash)
        .bind(&input.signing_key)    // Ed25519 public key
        .bind(&None::<String>) // last_jti starts as None
        .bind(false) // allow_anonymous_messages
        .bind(false) // require_proof_verification
        .bind(true) // is_active
        .bind(is_platform_attested) // is_platform_attested
        .fetch_one(exec.clone())
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Client identifier already exists".into())
            }
            _ => VaultlessError::Database(e),
        })?;

        // Step 6: Create session token
        let ttl_seconds = SESSION_DURATION_HOURS * 3600;
        let key_manager = session_verifier.key_manager();
        let integrity_handler = app_resolved.integrity()?;

        let session_data = SessionData {
            client_id: client.id,
            application_id: client.application_id,
            platform: input.platform_data.platform().as_str().to_string(),
            device_trust_score,
            platform_config_version: integrity_handler
                .platform_config_version
                .get(input.platform_data.platform()),
            app_tier: Some(app_resolved.sub_tier.to_string()),
            application_secret_api_key_id: Some(app_resolved.sk_id),
            pubkey: Some(input.signing_key.clone()),  // Store signing key in session
        };

        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        let (_, jti) = verify_session_token(&key_manager, &session_token)?;
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // Step 7: Update client with JTI
        sqlx::query!(
            "UPDATE clients SET last_jti = $1 WHERE id = $2",
            &jti,
            client.id
        )
        .execute(exec)
        .await?;

        // Step 8: Cache client in Redis
        let _ = Self::cache_to_redis(&redis, &client).await;

        tracing::info!(
            client_id = %client.id,
            application_id = %client.application_id,
            platform = %input.platform_data.platform().as_str(),
            is_platform_attested = %is_platform_attested,
            device_trust_score = %device_trust_score,
            "Client registered successfully"
        );

        Ok(SignupClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
        })
    }

    /// Register new client with platform attestation using HybridSessionVerifier
    pub async fn sign_up_hybrid<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        hybrid_verifier: Arc<HybridSessionVerifier>,
        app_resolved: Arc<ApplicationKeyView>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: SignupClientRequest,
        ip_address: std::net::IpAddr,
    ) -> Result<SignupClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // Step 1: Verify challenge
        Self::verify_and_consume_challenge(&redis, &input.challenge).await?;

        // Step 2: Verify challenge signature using Ed25519 signing key
        let signing_key = input.signing_key.clone();
        Self::verify_signup_challenge_signature(
            &signing_key,
            &input.challenge,
            &input.challenge_signed,
        )?;

        // Step 3: Platform attestation
        let detected_platform = input.platform_data.platform();
        let attestation_result = if let Some(integrity_svc) = integrity_service {

            let integrity_handler = app_resolved.integrity()?;
            let (attested_platform, attestation_result) = integrity_svc
                .verify_integrity(
                    &AttestationRequest {
                        platform_data: input.platform_data,
                    },
                    Some(&input.challenge),
                    &integrity_handler.config,
                    app_resolved.app_id,
                    None, // No client_id available during registration
                    Some(ip_address),
                )
                .await?;

            if attested_platform != detected_platform {
                return Err(VaultlessError::Unauthorized(
                    "Platform mismatch during attestation".into(),
                ));
            }

            Some(attestation_result)
        } else {
            None
        };

        // Step 4: Generate unique client identifier hash if provided
        let client_identifier_hash = input
            .client_identifier
            .as_ref()
            .map(|ci| crypto::hash_content(ci.as_bytes()));

        // Step 5: Create new client in database
        let client_id = Uuid::new_v4();
        let device_trust_score = attestation_result
            .as_ref()
            .map(|ar| ar.trust_score_percent)
            .unwrap_or(0);
        let is_platform_attested = attestation_result.is_some();

        let client = sqlx::query_as::<_, Client>(&format!(
            "INSERT INTO clients ({}) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,NOW(),$9) RETURNING *",
            MINIMAL_CLIENT_FIELDS
        ))
        .bind(client_id)
        .bind(&input.identifier)
        .bind(&client_identifier_hash)
        .bind(&input.signing_key)    // Ed25519 public key
        .bind(&None::<String>) // last_jti starts as None
        .bind(false) // allow_anonymous_messages
        .bind(false) // require_proof_verification
        .bind(true) // is_active
        .bind(is_platform_attested) // is_platform_attested
        .fetch_one(exec.clone())
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Client identifier already exists".into())
            }
            _ => VaultlessError::Database(e),
        })?;

        // Step 6: Create session token
        let ttl_seconds = SESSION_DURATION_HOURS * 3600;
        let key_manager = hybrid_verifier.key_manager();
        let integrity_handler = app_resolved.integrity()?;

        let session_data = SessionData {
            client_id: client.id,
            application_id: client.application_id,
            platform: detected_platform.to_string(),
            device_trust_score,
            platform_config_version: integrity_handler
                .platform_config_version
                .get(detected_platform),
            app_tier: Some(app_resolved.sub_tier.to_string()),
            application_secret_api_key_id: Some(app_resolved.sk_id),
            pubkey: Some(input.signing_key.clone()),  // Store signing key in session
        };

        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        let (_, jti) = verify_session_token(&key_manager, &session_token)?;
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // Step 7: Update client with JTI
        sqlx::query!(
            "UPDATE clients SET last_jti = $1 WHERE id = $2",
            &jti,
            client.id
        )
        .execute(exec)
        .await?;

        // Step 8: Cache client in Redis
        let _ = Self::cache_to_redis(&redis, &client).await;

        tracing::info!(
            client_id = %client.id,
            application_id = %client.application_id,
            platform = %detected_platform.as_str(),
            is_platform_attested = %is_platform_attested,
            device_trust_score = %device_trust_score,
            "Client registered successfully (HybridSessionVerifier)"
        );

        Ok(SignupClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
        })
    }

    /// Verify the signup challenge signature using Ed25519
    fn verify_signup_challenge_signature(
        signing_key: &str,
        challenge: &str,
        challenge_signature: &str,
    ) -> Result<()> {
        let is_valid =
            crate::crypto::verify_signature(challenge.as_bytes(), challenge_signature, signing_key)
                .is_ok();
        if !is_valid {
            return Err(VaultlessError::Unauthorized(
                "Invalid challenge signature".into(),
            ));
        }
        Ok(())
    }
}
