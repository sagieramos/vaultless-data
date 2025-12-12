use super::dto::*;
use crate::{ApplicationKeyView, cache_key};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::{AsyncCommands, Script};
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
                AttestationService, Platform, check_attestation_rate_limit, dto::IntegrityConfig,
                track_failed_attestation,
            },
            dto::Application,
        },
        session::paseto_session::{self, SessionData, SessionKeyManager, verify_session_token},
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
        attestation_service: Option<Arc<AttestationService>>,
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

        // ============= 5. PLATFORM ATTESTATION =============

        // ============= END ATTESTATION =============

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
            platform: platform_string,
            device_trusted,
            app_tier: None,
            application_secret_api_key_id: None,
            pubkey: Some(pubkey.clone()),
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
        .bind(&pubkey)
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
            expires_at,
        })
    }
}
