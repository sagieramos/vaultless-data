use super::dto::*;
use crate::cache_key;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    crypto,
    error::{Result, VaultlessError},
};

const CHALLENGE_EXPIRY_SECONDS: u64 = 300; // 5 minutes

impl Client {
    /// Verify session token validity
    pub async fn verify_session<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        session_token: &str,
    ) -> Result<Client>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let token_bytes = BASE64
            .decode(session_token)
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;
        let session_token_hash = crypto::hash_content(&token_bytes);
        let cache_key = cache_client_session_key(&session_token_hash);

        // --- 1. Redis Cache Lookup ---
        if let Some(redis_pool) = &redis
            && let Ok(mut conn) = redis_pool.get().await
            && let Ok(cached_json) = conn.get::<_, String>(&cache_key).await
            && let Ok(cached_client) = serde_json::from_str::<Client>(&cached_json)
        {
            tracing::debug!("Cache hit for client session");
            return Ok(cached_client);
        }

        // --- 2. Database Lookup ---
        let client = sqlx::query_as::<_, Client>(
            r#"
        SELECT * FROM clients
        WHERE session_token_hash = $1
          AND session_expires_at > NOW()
          AND is_active = TRUE
        "#,
        )
        .bind(&session_token_hash)
        .fetch_optional(exec)
        .await?
        .ok_or_else(|| VaultlessError::Unauthorized("Invalid or expired session".to_string()))?;

        let client_clone = client.clone();

        // --- 3. Write-Back to Redis ---
        if let Some(redis_pool) = &redis
            && let Ok(mut conn) = redis_pool.get().await
            && let Some(expiry) = client_clone.session_expires_at
        {
            let ttl_secs = (expiry - Utc::now()).num_seconds().max(0);
            if ttl_secs > 0 {
                let serialized = serde_json::to_string(&client_clone)?;
                if let Err(e) = conn
                    .set_ex::<_, _, ()>(&cache_key, serialized, ttl_secs as u64)
                    .await
                {
                    tracing::warn!("Redis cache write failed: {}", e);
                }
            }
        }

        Ok(client_clone)
    }

    /// Log out a client by clearing session
    pub async fn revoke_session<'c, E>(
        exec: E,
        redis: Option<&Arc<RedisPool>>,
        client_id: Uuid,
        session_token: Option<&str>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query(
            r#"
        UPDATE clients
        SET session_token_hash = NULL,
            session_expires_at = NULL
        WHERE id = $1
        "#,
        )
        .bind(client_id)
        .execute(exec)
        .await?;

        // --- Invalidate Redis Cache ---
        if let (Some(redis), Some(token)) = (redis, session_token) {
            let token_bytes = BASE64
                .decode(token)
                .map_err(|e| VaultlessError::Validation(e.to_string()))?;
            let session_hash = crypto::hash_content(&token_bytes);
            let cache_key = cache_client_session_key(&session_hash);

            if let Ok(mut conn) = redis.get().await {
                let _ = conn.del::<_, ()>(&cache_key).await.ok();
                tracing::debug!("Revoked session cache for client {}", client_id);
            }
        }

        Ok(())
    }

    /// Deactivate client (manual or cleanup trigger)
    pub async fn deactivate<'c, E>(
        exec: E,
        redis: Option<&Arc<RedisPool>>,
        client_id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // --- 1. Fetch client data before deactivation (for cache invalidation) ---
        let client = sqlx::query_as::<_, Client>("SELECT * FROM clients WHERE id = $1")
            .bind(client_id)
            .fetch_optional(exec.clone())
            .await?
            .ok_or_else(|| VaultlessError::NotFound("Client not found".into()))?;

        // --- 2. Deactivate in database ---
        sqlx::query("UPDATE clients SET is_active = FALSE WHERE id = $1")
            .bind(client_id)
            .execute(exec)
            .await?;

        // --- 3. Invalidate Redis caches (all possible keys) ---
        if let Some(redis_pool) = redis
            && let Ok(mut conn) = redis_pool.get().await
        {
            let mut keys_to_delete = Vec::new();

            // Canonical client data key
            keys_to_delete.push(cache_key!("client", "id", client.id));

            // Alias keys (if they exist)
            if let Some(ref pk) = client.public_key {
                keys_to_delete.push(cache_key!("client", "alias", "public_key", pk));
            }
            if let Some(ref idf) = client.identifier {
                keys_to_delete.push(cache_key!("client", "alias", "identifier", idf));
            }
            if let Some(ref cid_hash) = client.client_identifier_hash {
                keys_to_delete.push(cache_key!(
                    "client",
                    "alias",
                    "client_identifier_hash",
                    cid_hash
                ));
            }

            // Session key (if exists)
            if let Some(ref session_hash) = client.session_token_hash {
                keys_to_delete.push(cache_client_session_key(session_hash));
            }

            // Delete all keys
            for key in keys_to_delete {
                let _ = conn.del::<_, ()>(&key).await;
                tracing::debug!("Invalidated cache key: {}", key);
            }
        }

        tracing::info!(client_id = %client_id, "Client deactivated successfully");

        Ok(())
    }

    /// Generate a temporary authentication challenge, cache its hash, and return it.
    pub async fn generate_and_cache_challenge(
        redis: Arc<RedisPool>,
    ) -> Result<AuthenticationChallenge> {
        let raw_bytes: [u8; 32] = crypto::generate_secure_token::<32>()?;
        let challenge_string = BASE64.encode(raw_bytes);
        let expires_at = Utc::now() + Duration::seconds(CHALLENGE_EXPIRY_SECONDS as i64);

        // Hash the challenge for storage
        let challenge_hash = crypto::hash_content(challenge_string.as_bytes());
        let cache_key = cache_auth_challenge_key(&challenge_hash);

        // Get connection and cache it
        let mut conn = redis.get().await?;
        conn.set_ex::<_, _, ()>(&cache_key, "1", CHALLENGE_EXPIRY_SECONDS)
            .await?;

        tracing::debug!("Cached new auth challenge. Key: {}", cache_key);

        Ok(AuthenticationChallenge {
            challenge: challenge_string, // Return the *original* string
            expires_at,
        })
    }

    /// Verify signed challenge (Ed25519/P-256/etc)
    pub fn verify_signature(&self, challenge: &str, signature: &str) -> Result<bool> {
        let Some(pubkey_b64) = &self.public_key else {
            return Err(VaultlessError::Validation(
                "Client has no registered public key".into(),
            ));
        };

        let message = challenge.as_bytes();
        match crate::crypto::verify_signature(message, signature, pubkey_b64) {
            Ok(()) => Ok(true),
            Err(VaultlessError::SignatureVerificationFailed) => Ok(false),
            Err(e) => Err(e),
        }
    }

    // =============================================================================
    // Cache Invalidation Helpers
    // =============================================================================

    /// Invalidate cached client session (after logout, session rotate, or revoke)
    pub async fn invalidate_client_session_cache(
        redis: &Arc<RedisPool>,
        session_token: &str,
    ) -> Result<()> {
        let token_bytes = BASE64
            .decode(session_token)
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;
        let session_hash = crypto::hash_content(&token_bytes);
        let cache_key = cache_client_session_key(&session_hash);

        if let Ok(mut conn) = redis.get().await
            && conn.del::<_, ()>(&cache_key).await.is_ok()
        {
            tracing::debug!("Invalidated session cache: {}", cache_key);
        }

        Ok(())
    }

    pub async fn resolve_client<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        public_key: Option<&str>,
        identifier: Option<&str>,
        client_identifier: Option<&str>,
    ) -> Result<Option<Client>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        if public_key.is_none() && identifier.is_none() && client_identifier.is_none() {
            return Err(VaultlessError::Validation(
                "At least one of public_key, identifier, or client_identifier must be provided"
                    .into(),
            ));
        }

        let client_identifier_hash =
            client_identifier.map(|cid| crypto::hash_content(cid.as_bytes()));

        // --- 1. Check Redis alias keys ---
        if let Some(redis_pool) = &redis {
            let mut conn = redis_pool.get().await?;
            let alias_keys = vec![
                public_key.map(|pk| cache_key!("client", "alias", "public_key", pk)),
                identifier.map(|idf| cache_key!("client", "alias", "identifier", idf)),
                client_identifier_hash
                    .as_ref()
                    .map(|h| cache_key!("client", "alias", "client_identifier_hash", h)),
            ];

            for key in alias_keys.into_iter().flatten() {
                if let Ok(client_id_str) = conn.get::<_, String>(&key).await
                    && let Ok(client_id) = Uuid::parse_str(&client_id_str)
                {
                    let client_cache_key = cache_key!("client", "id", client_id);
                    if let Ok(cached_json) = conn.get::<_, String>(&client_cache_key).await
                        && let Ok(cached_client) = serde_json::from_str::<Client>(&cached_json)
                    {
                        tracing::debug!("Cache hit for client_id {}", client_id);
                        return Ok(Some(cached_client));
                    }
                }
            }
        }

        // --- 2. Database Lookup (minimal fields) ---
        let client = if let Some(pk) = public_key {
            sqlx::query_as::<_, Client>(&format!(
                "SELECT {} FROM clients WHERE public_key = $1 AND is_active = TRUE",
                MINIMAL_CLIENT_FIELDS
            ))
            .bind(pk)
            .fetch_optional(exec)
            .await?
        } else if let Some(idf) = identifier {
            sqlx::query_as::<_, Client>(&format!(
                "SELECT {} FROM clients WHERE identifier = $1 AND is_active = TRUE",
                MINIMAL_CLIENT_FIELDS
            ))
            .bind(idf)
            .fetch_optional(exec)
            .await?
        } else if let Some(cid_hash) = &client_identifier_hash {
            sqlx::query_as::<_, Client>(&format!(
                "SELECT {} FROM clients WHERE client_identifier_hash = $1 AND is_active = TRUE",
                MINIMAL_CLIENT_FIELDS
            ))
            .bind(cid_hash)
            .fetch_optional(exec)
            .await?
        } else {
            None
        };

        let client = match client {
            Some(c) => c,
            None => return Ok(None),
        };

        // --- 3. Cache aliases + canonical client ---
        if let Some(redis_pool) = &redis {
            let _ = Self::cache_to_redis(redis_pool, &client).await;
        }

        Ok(Some(client))
    }
}
