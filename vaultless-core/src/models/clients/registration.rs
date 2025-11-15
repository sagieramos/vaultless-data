// Add these imports to your existing client.rs file
use super::dto::*;
use crate::cache_key;
use crate::models::app_model::attestation::{self, verify_attestation};
use crate::models::app_model::attestation_types::*;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::{AsyncCommands, Script};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::app_model::dto::Application,
};

// Update RegisterClientRequest struct
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RegisterClientRequest {
    /// Public key or device fingerprint (client-side hash input)
    pub client_identifier: Option<String>,

    /// Optional: public key for signature verification (E2EE)
    #[validate(length(min = 32, max = 1024))]
    pub public_key: Option<String>,

    /// Optional: short shareable identifier (if user wants a vanity name)
    #[validate(length(min = 3, max = 64))]
    pub identifier: Option<String>,

    /// Optional: encrypted metadata (device info, version, etc.)
    pub metadata: Option<serde_json::Value>,

    /// Optional: signature proving ownership of the provided public_key.
    /// When present, server will verify signature against `signed_payload`.
    #[validate(length(min = 16, max = 2048))]
    pub signature: Option<String>,

    /// Optional: arbitrary payload that was signed (recommended: client_identifier or timestamp)
    pub signed_payload: Option<String>,

    /// Optional: nonce for replay protection — server will check Redis for reuse.
    #[validate(length(min = 8, max = 128))]
    pub nonce: Option<String>,

    // ============= NEW ATTESTATION FIELDS =============
    /// Platform attestation request (iOS/Android only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<AttestationRequest>,
}

// Updated Client::register() method
impl Client {
    /// Register new client with optional platform attestation
    pub async fn register<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        input: RegisterClientRequest,
        publishable_key: String,
        // NEW: Optional Google Cloud credentials for Android attestation
        google_cloud_project: Option<String>,
        google_api_key: Option<String>,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
    {
        // --- Validate input ---
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

        // --- Verify signature ---
        crate::crypto::verify_signature(payload.as_bytes(), &signature, &pubkey)
            .map_err(|_| VaultlessError::Validation("Signature verification failed".into()))?;

        // --- Nonce replay protection ---
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

        let app = Application::fetch_auth_config_by_publishable_key(
            exec.clone(),
            redis.clone(),
            &publishable_key,
        )
        .await?
        .ok_or(VaultlessError::NotFound(
            "Auth configuration not found".to_string(),
        ))?;

        // ============= NEW: PLATFORM ATTESTATION =============

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
                "Verifying platform attestation"
            );

            // Verify attestation against application's integrity_config
            let attestation_result = verify_attestation(
                &attestation_request,
                &app.app_integrity_config,
                google_cloud_project.as_deref(),
                google_api_key.as_deref(),
            )
            .await?;

            // Enforce attestation policies
            attestation::enforce_attestation_policies(
                &attestation_result,
                &app.app_integrity_config,
            )?;

            // Create attestation metadata
            let attestation_meta = AttestationMetadata::from_result(
                attestation_result,
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
                device_trusted = attestation_meta.is_device_trusted(),
                "Platform attestation successful"
            );
        }

        // ============= END ATTESTATION =============

        // --- Compute identifier hash (if provided) ---
        let client_identifier_hash = input
            .client_identifier
            .as_ref()
            .map(|ci| crypto::hash_content(ci.as_bytes()));

        // --- Session token ---
        let token = crypto::generate_secure_token::<32>()?;
        let session_token = BASE64.encode(token);
        let session_token_hash = crypto::hash_content(&token);
        let session_expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS);

        // --- Insert client into DB ---
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
        .bind(app.app_user_id)
        .bind(app.app_id)
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
            application_id = %app.app_id,
            developer_id = %app.app_user_id,
            is_platform_attested = %is_platform_attested,
            "Client registered successfully"
        );

        // --- Cache in Redis (non-critical) ---
        if let Some(pool) = &redis {
            let _ = Self::cache_to_redis(pool, &client).await;
        }

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at: session_expires_at,
        })
    }

    // ============= NEW: RE-ATTESTATION METHOD =============

    /// Re-attest an existing client (for periodic verification)
    pub async fn re_attest<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        client_id: Uuid,
        attestation_request: AttestationRequest,
        google_cloud_project: Option<String>,
        google_api_key: Option<String>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // 1. Fetch existing client
        let client =
            sqlx::query_as::<_, Client>("SELECT * FROM clients WHERE id = $1 AND is_active = TRUE")
                .bind(client_id)
                .fetch_optional(exec.clone())
                .await?
                .ok_or_else(|| VaultlessError::NotFound("Client not found".into()))?;

        // 2. Get application config
        let app = sqlx::query_as::<_, Application>("SELECT * FROM applications WHERE id = $1")
            .bind(client.application_id)
            .fetch_optional(exec.clone())
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

        // 3. Validate attestation request
        attestation_request.validate().map_err(|e| {
            VaultlessError::Validation(format!("Invalid attestation request: {}", e))
        })?;

        // 4. Verify attestation
        let attestation_result = verify_attestation(
            &attestation_request,
            &app.integrity_config,
            google_cloud_project.as_deref(),
            google_api_key.as_deref(),
        )
        .await?;

        // 5. Enforce policies
        attestation::enforce_attestation_policies(&attestation_result, &app.integrity_config)?;

        // 6. Update client metadata
        let mut attestation_meta =
            AttestationMetadata::from_metadata(client.metadata.as_ref())?.unwrap_or_default();

        attestation_meta.update_from_result(attestation_result);

        let updated_metadata = attestation_meta.merge_into_metadata(client.metadata.clone())?;

        // 7. Update database
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
        .execute(exec)
        .await?;

        // 8. Invalidate cache
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

    // ============= HELPER: CHECK ATTESTATION STATUS =============

    /// Check if client needs re-attestation (based on age)
    pub fn needs_reattesation(&self, days: i64) -> bool {
        if !self.is_platform_attested {
            return false; // Client never attested, not required
        }

        match AttestationMetadata::from_metadata(self.metadata.as_ref()) {
            Ok(Some(meta)) => meta.needs_reattesation(days),
            _ => true, // If we can't parse metadata, assume re-attestation needed
        }
    }

    /// Get attestation metadata from client
    pub fn get_attestation_metadata(&self) -> Result<Option<AttestationMetadata>> {
        AttestationMetadata::from_metadata(self.metadata.as_ref())
    }
}
