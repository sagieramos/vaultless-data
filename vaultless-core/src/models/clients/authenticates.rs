use super::dto::*;
use crate::cache_key;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::PgPool;
use sqlx::{Executor, Postgres};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::session::paseto_session::{
        SessionData, SessionKeyManager, revoke_session, verify_and_check_revocation_atomic,
        verify_session_token,
    },
};

const CHALLENGE_EXPIRY_SECONDS: u64 = 300; // 5 minutes
const DEFAULT_REVOCATION_TTL: u64 = 24 * 30 * 3600; // 30 days (fallback for force logout)

impl Client {
    pub async fn fetch_active_client(
        db_pool: &PgPool,
        redis: &Arc<RedisPool>,
        client_id: Uuid,
    ) -> Result<Client> {
        let client_cache_key = cache_key!("client", "id", client_id);
        if let Ok(mut conn) = redis.get().await
            && let Ok(cached_json) = conn.get::<_, String>(&client_cache_key).await
            && let Ok(cached_client) = serde_json::from_str::<Client>(&cached_json)
        {
            if !cached_client.is_active {
                return Err(VaultlessError::Unauthorized("Client is deactivated".into()));
            }
            tracing::debug!("Cache hit for client {}", client_id);
            return Ok(cached_client);
        }

        // --- 2. Database Lookup (Cache Miss) ---
        let client = sqlx::query_as::<_, Client>(
            r#"SELECT * FROM clients WHERE id = $1 AND is_active = TRUE"#,
        )
        .bind(client_id)
        .fetch_optional(db_pool)
        .await?
        .ok_or_else(|| VaultlessError::Unauthorized("Client not found or inactive".to_string()))?;

        let redis_for_cache = redis.clone();
        let client_for_cache = client.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::cache_to_redis(&redis_for_cache, &client_for_cache).await {
                tracing::warn!(
                    "Background cache update failed for client {}: {}",
                    client_for_cache.id,
                    e
                );
            }
        });

        Ok(client)
    }

    pub async fn verify_session_fast(
        redis: Arc<RedisPool>,
        key_manager: &SessionKeyManager,
        session_token: &str,
    ) -> Result<SessionData> {
        let session_data =
            verify_and_check_revocation_atomic(key_manager, &redis, session_token).await?;

        Ok(session_data)
    }

    pub async fn revoke_client_session<'c, E>(
        exec: E,
        redis: Option<&Arc<RedisPool>>,
        key_manager: &SessionKeyManager,
        client_id: Uuid,
        session_token: Option<&str>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let redis_pool = if let Some(pool) = redis {
            pool
        } else {
            return Err(VaultlessError::Config(
                "Internal; session logout not possible".into(),
            ));
        };

        if let Some(token) = session_token {
            if let Ok((_data, jti)) = verify_session_token(key_manager, token) {
                revoke_session(redis_pool, &jti, DEFAULT_REVOCATION_TTL).await?;
                tracing::info!(client_id = %client_id, jti = %jti, "Explicit session revoked");

                sqlx::query("UPDATE clients SET last_jti = NULL WHERE id = $1 AND last_jti = $2")
                    .bind(client_id)
                    .bind(jti)
                    .execute(exec)
                    .await?;
            }
        } else {
            let killed_jti: Option<String> = sqlx::query_scalar(
                "UPDATE clients SET last_jti = NULL WHERE id = $1 RETURNING last_jti",
            )
            .bind(client_id)
            .fetch_optional(exec)
            .await?
            .flatten();

            if let Some(jti) = killed_jti {
                revoke_session(redis_pool, &jti, DEFAULT_REVOCATION_TTL).await?;
                tracing::info!(client_id = %client_id, jti = %jti, "Last active session revoked via DB lookup");
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
        // --- 1. Fetch client data before deactivation ---
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

        // --- 3. Invalidate Redis caches & Revoke Session ---
        if let Some(redis_pool) = redis
            && let Ok(mut conn) = redis_pool.get().await
        {
            // Revoke active session if exists
            if let Some(jti) = &client.last_jti {
                let _ = revoke_session(redis_pool, jti, DEFAULT_REVOCATION_TTL).await;
                tracing::info!(client_id = %client.id, jti = %jti, "Session revoked due to deactivation");
            }

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
                        // Ensure we don't return deactivated clients from cache
                        if cached_client.is_active {
                            tracing::debug!("Cache hit for client_id {}", client_id);
                            return Ok(Some(cached_client));
                        }
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
