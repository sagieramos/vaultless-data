use super::dto::*;
use crate::models::app_model::attestation::{self, verify_attestation};
use crate::models::app_model::attestation_types::{
    AttestationMetadata, Platform, get_google_credentials,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;

use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::app_model::dto::Application,
};

// =============================================================================
// Constants
// =============================================================================

const SESSION_DURATION_HOURS: i64 = 24 * 30; // 30 days

impl Client {
    /// Authenticate client by hashed identifier
    pub async fn authenticate<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        input: AuthenticateClientRequest,
    ) -> Result<AuthenticateClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let mut was_reattested = false;
        // --- 1. Atomically check and consume the challenge from Redis ---
        let challenge_hash = crypto::hash_content(input.challenge.as_bytes());
        let cache_key = cache_auth_challenge_key(&challenge_hash);
        let mut conn = redis.get().await?;

        // Use GETDEL: Get the value and delete the key atomically.
        // If it returns None (null), the key didn't exist (invalid, expired, or replayed).
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
        let client = if let Some(ref hash) = input.client_identifier_hash {
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

        // --- 5. Check if re-attestation is required ---
        if client.needs_reattesation(30) {
            if input.attestation.is_none() {
                return Err(VaultlessError::Unauthorized(
                    "Re-attestation required. Please provide attestation token.".into(),
                ));
            }
        }

        // --- 6. If attestation provided, verify it ---
        if let Some(attestation_request) = input.attestation {
            was_reattested = true;
            // Fetch application to get integrity_config
            let app = sqlx::query_as::<_, Application>("SELECT * FROM applications WHERE id = $1")
                .bind(client.application_id)
                .fetch_optional(exec.clone())
                .await?
                .ok_or_else(|| VaultlessError::NotFound("Application not found".into()))?;

            // Verify attestation (same as in register)
            let (google_project, google_key) = if attestation_request.platform == Platform::Android
            {
                get_google_credentials(&app.integrity_config).ok_or_else(|| {
                    VaultlessError::Internal("Google credentials not configured".into())
                })?
            } else {
                (String::new(), String::new())
            };

            let attestation_result = verify_attestation(
                &attestation_request,
                &app.integrity_config,
                if attestation_request.platform == Platform::Android {
                    Some(&google_project)
                } else {
                    None
                },
                if attestation_request.platform == Platform::Android {
                    Some(&google_key)
                } else {
                    None
                },
            )
            .await?;

            attestation::enforce_attestation_policies(&attestation_result, &app.integrity_config)?;

            // Update client metadata with new attestation
            let mut attestation_meta =
                AttestationMetadata::from_metadata(client.metadata.as_ref())?.unwrap_or_default();
            attestation_meta.update_from_result(attestation_result);
            let updated_metadata = attestation_meta.merge_into_metadata(client.metadata.clone())?;

            sqlx::query(
                "UPDATE clients SET metadata = $1, is_platform_attested = TRUE WHERE id = $2",
            )
            .bind(&updated_metadata)
            .bind(client.id)
            .execute(exec.clone())
            .await?;

            tracing::info!(
                client_id = %client.id,
                platform = %attestation_request.platform,
                "Re-attestation successful during authentication"
            );
        }

        // --- 7. Verify the signed challenge ---
        // We verify the *original challenge string* against the signature.
        if !client.verify_signature(&input.challenge, &input.challenge_signature)? {
            return Err(VaultlessError::Unauthorized(
                "Invalid challenge signature".into(),
            ));
        }

        // --- 8. Generate new session token (always fresh, no expiry check) ---
        let old_session_hash = client.session_token_hash.clone(); // For cache invalidation
        let token = crypto::generate_secure_token::<32>()?;
        let session_token = BASE64.encode(token);
        let session_token_hash = crypto::hash_content(&token);
        let expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS);

        sqlx::query(
            r#"
        UPDATE clients
        SET session_token_hash = $1,
            session_expires_at = $2,
            last_seen_at = NOW()
        WHERE id = $3
        "#,
        )
        .bind(&session_token_hash)
        .bind(expires_at)
        .bind(client.id)
        .execute(exec)
        .await?;

        // --- Invalidate old session key from cache ---
        if let Some(old_hash) = old_session_hash {
            let old_cache_key = cache_client_session_key(&old_hash);
            let _ = conn.del::<_, ()>(&old_cache_key).await;
            tracing::debug!("Invalidated old session cache key: {}", old_cache_key);
        }

        Ok(AuthenticateClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
            is_new_session: true,
            was_reattested,
        })
    }
}
