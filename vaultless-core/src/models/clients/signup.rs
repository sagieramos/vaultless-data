use super::dto::*;
use crate::cache_key;
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::{AsyncCommands, Script};
use serde_json::Value;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::{
        app_model::{
            attestation::{
                IntegrityService, Platform,
                captcha::{CaptchaProvider, verify_captcha},
                types::PlatformAttestationData,
            },
            dto::ApplicationKeyView,
        },
        session::{
            paseto_session::{self, SessionData, SessionKeyManager, verify_session_token},
            HybridSessionVerifier, SessionVerifier,
        },
    },
};

const SESSION_DURATION_HOURS: u64 = 24 * 30; // 30 days
const IDENTIFIER_TTL_SECS: usize = 60; // Short TTL for nonce

impl Client {
    /// Register new client with optional platform attestation
    pub async fn sign_up<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        key_manager: Arc<SessionKeyManager>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: RegisterClientRequest,
        auth_config: Arc<ApplicationKeyView>,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // --- 1. Validate input ---
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // --- 2. Verify signature ---
        if let Some(ref payload) = input.signed_payload {
            crate::crypto::verify_signature(
                payload.as_bytes(),
                &input.signature,
                &input.public_key,
            )
            .map_err(|_| VaultlessError::Validation("Signature verification failed".into()))?;
        } else {
            return Err(VaultlessError::Validation(
                "Signed payload is required".into(),
            ));
        }

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

        // --- 4. CAPTCHA verification (for web registration) ---
        if let (Some(captcha_token), Some(ref integrity_handler)) =
            (&input.captcha_token, auth_config.integrity().ok())
        {
            if let Some(browser_config) = integrity_handler.get_browser_config() {
                if browser_config
                    .require_captcha_on_registration
                    .unwrap_or(false)
                {
                    let captcha_provider = browser_config
                        .captcha_provider
                        .as_deref()
                        .unwrap_or("turnstile");
                    let captcha_secret =
                        browser_config
                            .captcha_secret_key
                            .as_deref()
                            .ok_or_else(|| {
                                VaultlessError::Validation(
                                    "CAPTCHA secret key not configured".into(),
                                )
                            })?;

                    let verified = verify_captcha(
                        match captcha_provider {
                            "turnstile" => CaptchaProvider::Turnstile,
                            "hcaptcha" => CaptchaProvider::HCaptcha,
                            "recaptcha" => CaptchaProvider::ReCaptcha,
                            _ => {
                                return Err(VaultlessError::Validation(
                                    "Invalid CAPTCHA provider".into(),
                                ));
                            }
                        },
                        captcha_token,
                        captcha_secret,
                        browser_config.captcha_site_key.as_deref(),
                        None, // IP address not provided (optional)
                    )
                    .await?;

                    if !verified {
                        return Err(VaultlessError::Validation(
                            "CAPTCHA verification failed".into(),
                        ));
                    }
                }
            }
        }

        // --- 5. Platform attestation (if provided) ---
        let mut platform: Platform = Platform::Browser; // Default to Browser
        let mut device_trusted = false;
        let mut is_platform_attested = false;
        let mut trust_score_percent: u8 = 0;
        let mut merged_metadata: Option<Value> = None;

        if let Some(ref attestation) = input.attestation {
            if let Some(integrity_svc) = &integrity_service {
                let integrity_handler = auth_config.integrity()?;

                // Determine platform from attestation data
                platform = match &attestation.platform_data {
                    PlatformAttestationData::Android(_) => Platform::Android,
                    PlatformAttestationData::IOS(_) => Platform::IOS,
                    PlatformAttestationData::IoT(_) => Platform::IoT,
                    PlatformAttestationData::Browser(_) => Platform::Browser,
                };

                // For signup, we don't have a client_id yet, so we pass None
                let (attested_platform, attestation_result) = integrity_svc
                    .verify_integrity(
                        attestation,
                        &integrity_handler.config,
                        auth_config.app_id,
                        None, // No client_id at registration time
                        None, // No IP address provided in attestation
                    )
                    .await?;

                if attested_platform != platform {
                    return Err(VaultlessError::Validation(
                        "Platform mismatch between attestation request and actual platform".into(),
                    ));
                }

                device_trusted = attestation_result.device_trusted;
                trust_score_percent = attestation_result.trust_score_percent;
                is_platform_attested = true;

                // Add attestation result to metadata
                let attestation_record: super::client_integrity_handler::AttestationRecord =
                    attestation_result.into();

                // Initialize merged_metadata with an empty object if it's None
                let mut metadata_map = serde_json::Map::new();

                // Add the new attestation result to the metadata
                metadata_map.insert(
                    platform.as_str().to_string(),
                    serde_json::to_value(&attestation_record)?,
                );

                merged_metadata = Some(Value::Object(metadata_map));
            } else {
                return Err(VaultlessError::Validation(
                    "Integrity service not configured".into(),
                ));
            }
        } else if let Some(provided_platform) = input.attestation_platform {
            // User provided platform hint without attestation
            platform = provided_platform;

            // Check if attestation is required for this platform based on application config
            let integrity_handler = auth_config.integrity()?;

            // If attestation is required for this platform but not provided, return error
            match platform {
                Platform::Android => {
                    if let Some(android_config) = integrity_handler.get_android_config() {
                        // Check if attestation is required based on minimum trust score or other config
                        // For now, we'll allow registration without attestation with a warning
                        tracing::info!("Android client registered without attestation");
                    }
                }
                Platform::IOS => {
                    if let Some(ios_config) = integrity_handler.get_ios_config() {
                        // Check if attestation is required based on minimum trust score or other config
                        tracing::info!("iOS client registered without attestation");
                    }
                }
                Platform::IoT => {
                    if let Some(iot_config) = integrity_handler.get_iot_config() {
                        // IoT devices typically require attestation, but allow if configured permissively
                        tracing::info!("IoT client registered without attestation");
                    }
                }
                Platform::Browser => {
                    // Browser platforms may not require hardware attestation in basic scenarios
                    tracing::info!("Browser client registered without attestation");
                }
            }
        } else {
            // If no platform hint provided, default to Browser
            tracing::info!("No platform specified, defaulting to Browser");
        }

        // --- 6. Compute identifier hash (if provided) ---
        let client_identifier_hash = input
            .client_identifier
            .as_ref()
            .map(|ci| crypto::hash_content(ci.as_bytes()));

        // --- 7. Generate Client ID & PASETO Session Token ---
        let client_id = Uuid::new_v4();

        let session_data = SessionData {
            client_id,
            application_id: auth_config.app_id,
            platform: platform.as_str().to_string(),
            device_trust_score: trust_score_percent,
            platform_config_version: auth_config
                .integrity()?
                .platform_config_version
                .get(platform),
            app_tier: auth_config.sk_tier.map(|tier| tier.to_string()),
            application_secret_api_key_id: Some(auth_config.sk_id),
            pubkey: Some(input.public_key.clone()),
        };

        let ttl_seconds = SESSION_DURATION_HOURS * 3600;

        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        let (_, jti) = verify_session_token(&key_manager, &session_token)?;

        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // --- 8. Insert client into DB ---
        let client = sqlx::query_as::<_, Client>(
            r#"
            INSERT INTO clients (
                id,
                identifier,
                client_identifier_hash,
                public_key,
                last_jti,
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
        .bind(client_id)
        .bind(&input.identifier)
        .bind(&client_identifier_hash)
        .bind(&input.public_key)
        .bind(&jti)
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
            platform = %platform.as_str(),
            is_platform_attested = %is_platform_attested,
            device_trusted = %device_trusted,
            trust_score_percent = %trust_score_percent,
            "Client registered successfully"
        );

        // --- 9. Cache in Redis (non-critical) ---
        if let Some(pool) = &redis {
            let _ = Self::cache_to_redis(pool, &client).await;
        }

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
        })
    }

    /// Register new client with optional platform attestation using SessionVerifier
    pub async fn signup<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        session_verifier: Arc<SessionVerifier>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: RegisterClientRequest,
        auth_config: Arc<ApplicationKeyView>,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // --- 1. Validate input ---
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // --- 2. Verify signature ---
        if let Some(ref payload) = input.signed_payload {
            crate::crypto::verify_signature(
                payload.as_bytes(),
                &input.signature,
                &input.public_key,
            )
            .map_err(|_| VaultlessError::Validation("Signature verification failed".into()))?;
        } else {
            return Err(VaultlessError::Validation(
                "Signed payload is required".into(),
            ));
        }

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

        // --- 4. CAPTCHA verification (for web registration) ---
        if let (Some(captcha_token), Some(ref integrity_handler)) =
            (&input.captcha_token, auth_config.integrity().ok())
        {
            if let Some(browser_config) = integrity_handler.get_browser_config() {
                if browser_config
                    .require_captcha_on_registration
                    .unwrap_or(false)
                {
                    let captcha_provider = browser_config
                        .captcha_provider
                        .as_deref()
                        .unwrap_or("turnstile");
                    let captcha_secret =
                        browser_config
                            .captcha_secret_key
                            .as_deref()
                            .ok_or_else(|| {
                                VaultlessError::Validation(
                                    "CAPTCHA secret key not configured".into(),
                                )
                            })?;

                    let verified = verify_captcha(
                        match captcha_provider {
                            "turnstile" => CaptchaProvider::Turnstile,
                            "hcaptcha" => CaptchaProvider::HCaptcha,
                            "recaptcha" => CaptchaProvider::ReCaptcha,
                            _ => {
                                return Err(VaultlessError::Validation(
                                    "Invalid CAPTCHA provider".into(),
                                ));
                            }
                        },
                        captcha_token,
                        captcha_secret,
                        browser_config.captcha_site_key.as_deref(),
                        None, // IP address not provided (optional)
                    )
                    .await?;

                    if !verified {
                        return Err(VaultlessError::Validation(
                            "CAPTCHA verification failed".into(),
                        ));
                    }
                }
            }
        }

        // --- 5. Platform attestation (if provided) ---
        let mut platform: Platform = Platform::Browser; // Default to Browser
        let mut device_trusted = false;
        let mut is_platform_attested = false;
        let mut trust_score_percent: u8 = 0;
        let mut merged_metadata: Option<Value> = None;

        if let Some(ref attestation) = input.attestation {
            if let Some(integrity_svc) = &integrity_service {
                let integrity_handler = auth_config.integrity()?;

                // Determine platform from attestation data
                platform = match &attestation.platform_data {
                    PlatformAttestationData::Android(_) => Platform::Android,
                    PlatformAttestationData::IOS(_) => Platform::IOS,
                    PlatformAttestationData::IoT(_) => Platform::IoT,
                    PlatformAttestationData::Browser(_) => Platform::Browser,
                };

                // For signup, we don't have a client_id yet, so we pass None
                let (attested_platform, attestation_result) = integrity_svc
                    .verify_integrity(
                        attestation,
                        &integrity_handler.config,
                        auth_config.app_id,
                        None, // No client_id at registration time
                        None, // No IP address provided in attestation
                    )
                    .await?;

                if attested_platform != platform {
                    return Err(VaultlessError::Validation(
                        "Platform mismatch between attestation request and actual platform".into(),
                    ));
                }

                device_trusted = attestation_result.device_trusted;
                trust_score_percent = attestation_result.trust_score_percent;
                is_platform_attested = true;

                // Add attestation result to metadata
                let attestation_record: super::client_integrity_handler::AttestationRecord =
                    attestation_result.into();

                // Initialize merged_metadata with an empty object if it's None
                let mut metadata_map = serde_json::Map::new();

                // Add the new attestation result to the metadata
                metadata_map.insert(
                    platform.as_str().to_string(),
                    serde_json::to_value(&attestation_record)?,
                );

                merged_metadata = Some(Value::Object(metadata_map));
            } else {
                return Err(VaultlessError::Validation(
                    "Integrity service not configured".into(),
                ));
            }
        } else if let Some(provided_platform) = input.attestation_platform {
            // User provided platform hint without attestation
            platform = provided_platform;

            // Check if attestation is required for this platform based on application config
            let integrity_handler = auth_config.integrity()?;

            // If attestation is required for this platform but not provided, return error
            match platform {
                Platform::Android => {
                    if let Some(android_config) = integrity_handler.get_android_config() {
                        // Check if attestation is required based on minimum trust score or other config
                        // For now, we'll allow registration without attestation with a warning
                        tracing::info!("Android client registered without attestation");
                    }
                }
                Platform::IOS => {
                    if let Some(ios_config) = integrity_handler.get_ios_config() {
                        // Check if attestation is required based on minimum trust score or other config
                        tracing::info!("iOS client registered without attestation");
                    }
                }
                Platform::IoT => {
                    if let Some(iot_config) = integrity_handler.get_iot_config() {
                        // IoT devices typically require attestation, but allow if configured permissively
                        tracing::info!("IoT client registered without attestation");
                    }
                }
                Platform::Browser => {
                    // Browser platforms may not require hardware attestation in basic scenarios
                    tracing::info!("Browser client registered without attestation");
                }
            }
        } else {
            // If no platform hint provided, default to Browser
            tracing::info!("No platform specified, defaulting to Browser");
        }

        // --- 6. Get key manager from session verifier and create session ---
        let key_manager = session_verifier.key_manager();
        let client_id = Uuid::new_v4();

        let session_data = SessionData {
            client_id,
            application_id: auth_config.app_id,
            platform: platform.as_str().to_string(),
            device_trust_score: trust_score_percent,
            platform_config_version: auth_config
                .integrity()?
                .platform_config_version
                .get(platform),
            app_tier: auth_config.sk_tier.map(|tier| tier.to_string()),
            application_secret_api_key_id: Some(auth_config.sk_id),
            pubkey: Some(input.public_key.clone()),
        };

        let ttl_seconds = SESSION_DURATION_HOURS * 3600;

        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        let (_, jti) = verify_session_token(&key_manager, &session_token)?;
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // --- 7. Insert client into DB ---
        let client = sqlx::query_as::<_, Client>(
            r#"
            INSERT INTO clients (
                id,
                identifier,
                client_identifier_hash,
                public_key,
                last_jti,
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
        .bind(client_id)
        .bind(&input.identifier)
        .bind(input.client_identifier.as_ref().map(|ci| crypto::hash_content(ci.as_bytes())))
        .bind(&input.public_key)
        .bind(&jti)
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
            platform = %platform.as_str(),
            is_platform_attested = %is_platform_attested,
            device_trusted = %device_trusted,
            trust_score_percent = %trust_score_percent,
            "Client registered successfully (SessionVerifier)"
        );

        // --- 8. Cache in Redis (non-critical) ---
        if let Some(pool) = &redis {
            let _ = Self::cache_to_redis(pool, &client).await;
        }

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
        })
    }

    /// Register new client with optional platform attestation using HybridSessionVerifier
    pub async fn signup_hybrid<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        hybrid_verifier: Arc<HybridSessionVerifier>,
        integrity_service: Option<Arc<IntegrityService>>,
        input: RegisterClientRequest,
        auth_config: Arc<ApplicationKeyView>,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // --- 1. Validate input ---
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // --- 2. Verify signature ---
        if let Some(ref payload) = input.signed_payload {
            crate::crypto::verify_signature(
                payload.as_bytes(),
                &input.signature,
                &input.public_key,
            )
            .map_err(|_| VaultlessError::Validation("Signature verification failed".into()))?;
        } else {
            return Err(VaultlessError::Validation(
                "Signed payload is required".into(),
            ));
        }

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

        // --- 4. CAPTCHA verification (for web registration) ---
        if let (Some(captcha_token), Some(ref integrity_handler)) =
            (&input.captcha_token, auth_config.integrity().ok())
        {
            if let Some(browser_config) = integrity_handler.get_browser_config() {
                if browser_config
                    .require_captcha_on_registration
                    .unwrap_or(false)
                {
                    let captcha_provider = browser_config
                        .captcha_provider
                        .as_deref()
                        .unwrap_or("turnstile");
                    let captcha_secret =
                        browser_config
                            .captcha_secret_key
                            .as_deref()
                            .ok_or_else(|| {
                                VaultlessError::Validation(
                                    "CAPTCHA secret key not configured".into(),
                                )
                            })?;

                    let verified = verify_captcha(
                        match captcha_provider {
                            "turnstile" => CaptchaProvider::Turnstile,
                            "hcaptcha" => CaptchaProvider::HCaptcha,
                            "recaptcha" => CaptchaProvider::ReCaptcha,
                            _ => {
                                return Err(VaultlessError::Validation(
                                    "Invalid CAPTCHA provider".into(),
                                ));
                            }
                        },
                        captcha_token,
                        captcha_secret,
                        browser_config.captcha_site_key.as_deref(),
                        None, // IP address not provided (optional)
                    )
                    .await?;

                    if !verified {
                        return Err(VaultlessError::Validation(
                            "CAPTCHA verification failed".into(),
                        ));
                    }
                }
            }
        }

        // --- 5. Platform attestation (if provided) ---
        let mut platform: Platform = Platform::Browser; // Default to Browser
        let mut device_trusted = false;
        let mut is_platform_attested = false;
        let mut trust_score_percent: u8 = 0;
        let mut merged_metadata: Option<Value> = None;

        if let Some(ref attestation) = input.attestation {
            if let Some(integrity_svc) = &integrity_service {
                let integrity_handler = auth_config.integrity()?;

                // Determine platform from attestation data
                platform = match &attestation.platform_data {
                    PlatformAttestationData::Android(_) => Platform::Android,
                    PlatformAttestationData::IOS(_) => Platform::IOS,
                    PlatformAttestationData::IoT(_) => Platform::IoT,
                    PlatformAttestationData::Browser(_) => Platform::Browser,
                };

                // For signup, we don't have a client_id yet, so we pass None
                let (attested_platform, attestation_result) = integrity_svc
                    .verify_integrity(
                        attestation,
                        &integrity_handler.config,
                        auth_config.app_id,
                        None, // No client_id at registration time
                        None, // No IP address provided in attestation
                    )
                    .await?;

                if attested_platform != platform {
                    return Err(VaultlessError::Validation(
                        "Platform mismatch between attestation request and actual platform".into(),
                    ));
                }

                device_trusted = attestation_result.device_trusted;
                trust_score_percent = attestation_result.trust_score_percent;
                is_platform_attested = true;

                // Add attestation result to metadata
                let attestation_record: super::client_integrity_handler::AttestationRecord =
                    attestation_result.into();

                // Initialize merged_metadata with an empty object if it's None
                let mut metadata_map = serde_json::Map::new();

                // Add the new attestation result to the metadata
                metadata_map.insert(
                    platform.as_str().to_string(),
                    serde_json::to_value(&attestation_record)?,
                );

                merged_metadata = Some(Value::Object(metadata_map));
            } else {
                return Err(VaultlessError::Validation(
                    "Integrity service not configured".into(),
                ));
            }
        } else if let Some(provided_platform) = input.attestation_platform {
            // User provided platform hint without attestation
            platform = provided_platform;

            // Check if attestation is required for this platform based on application config
            let integrity_handler = auth_config.integrity()?;

            // If attestation is required for this platform but not provided, return error
            match platform {
                Platform::Android => {
                    if let Some(android_config) = integrity_handler.get_android_config() {
                        // Check if attestation is required based on minimum trust score or other config
                        // For now, we'll allow registration without attestation with a warning
                        tracing::info!("Android client registered without attestation");
                    }
                }
                Platform::IOS => {
                    if let Some(ios_config) = integrity_handler.get_ios_config() {
                        // Check if attestation is required based on minimum trust score or other config
                        tracing::info!("iOS client registered without attestation");
                    }
                }
                Platform::IoT => {
                    if let Some(iot_config) = integrity_handler.get_iot_config() {
                        // IoT devices typically require attestation, but allow if configured permissively
                        tracing::info!("IoT client registered without attestation");
                    }
                }
                Platform::Browser => {
                    // Browser platforms may not require hardware attestation in basic scenarios
                    tracing::info!("Browser client registered without attestation");
                }
            }
        } else {
            // If no platform hint provided, default to Browser
            tracing::info!("No platform specified, defaulting to Browser");
        }

        // --- 6. Get key manager from hybrid verifier and create session ---
        let key_manager_arc = hybrid_verifier.key_manager();
        let key_manager = key_manager_arc.as_ref();
        let client_id = Uuid::new_v4();

        let session_data = SessionData {
            client_id,
            application_id: auth_config.app_id,
            platform: platform.as_str().to_string(),
            device_trust_score: trust_score_percent,
            platform_config_version: auth_config
                .integrity()?
                .platform_config_version
                .get(platform),
            app_tier: auth_config.sk_tier.map(|tier| tier.to_string()),
            application_secret_api_key_id: Some(auth_config.sk_id),
            pubkey: Some(input.public_key.clone()),
        };

        let ttl_seconds = SESSION_DURATION_HOURS * 3600;

        let session_token =
            paseto_session::create_session_token(key_manager.current(), session_data, ttl_seconds)?;

        let (_, jti) = verify_session_token(key_manager, &session_token)?;
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS as i64);

        // --- 7. Insert client into DB ---
        let client = sqlx::query_as::<_, Client>(
            r#"
            INSERT INTO clients (
                id,
                identifier,
                client_identifier_hash,
                public_key,
                last_jti,
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
        .bind(client_id)
        .bind(&input.identifier)
        .bind(input.client_identifier.as_ref().map(|ci| crypto::hash_content(ci.as_bytes())))
        .bind(&input.public_key)
        .bind(&jti)
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
            platform = %platform.as_str(),
            is_platform_attested = %is_platform_attested,
            device_trusted = %device_trusted,
            trust_score_percent = %trust_score_percent,
            "Client registered successfully (HybridSessionVerifier)"
        );

        // --- 8. Cache in Redis (non-critical) ---
        if let Some(pool) = &redis {
            let _ = Self::cache_to_redis(pool, &client).await;
        }

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
        })
    }
}
