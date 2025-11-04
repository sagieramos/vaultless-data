use crate::cache_key;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use crate::crypto;
use crate::error::{Result, VaultlessError};

// =============================================================================
// Constants
// =============================================================================

const SESSION_DURATION_HOURS: i64 = 24 * 30; // 30 days
const CHALLENGE_EXPIRY_SECONDS: i64 = 300; // 5 minutes
const IDENTIFIER_TTL_SECS: u64 = 600; // 10 minutes

// =============================================================================
// Models
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Client {
    pub id: Uuid,
    pub identifier: Option<String>,
    pub client_identifier_hash: String,
    pub public_key: Option<String>,
    pub session_token_hash: Option<String>,
    pub session_expires_at: Option<DateTime<Utc>>,
    pub allow_anonymous_messages: bool,
    pub require_proof_verification: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub developer_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientPublic {
    pub id: Uuid,
    pub identifier: Option<String>,
    pub public_key: Option<String>,
    pub allow_anonymous_messages: bool,
    pub require_proof_verification: bool,
    pub is_active: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
}

impl From<Client> for ClientPublic {
    fn from(c: Client) -> Self {
        Self {
            id: c.id,
            identifier: c.identifier,
            public_key: c.public_key,
            allow_anonymous_messages: c.allow_anonymous_messages,
            require_proof_verification: c.require_proof_verification,
            is_active: c.is_active,
            last_seen_at: c.last_seen_at,
            last_message_at: c.last_message_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RegisterClientRequest {
    /// Public key or device fingerprint (client-side hash input)
    #[validate(length(min = 32, max = 1024))]
    pub client_identifier: String,

    /// Optional: public key for signature verification (E2EE)
    #[validate(length(min = 32, max = 1024))]
    pub public_key: Option<String>,

    /// Optional: short shareable identifier (if user wants a vanity name)
    #[validate(length(min = 3, max = 64))]
    pub identifier: Option<String>,

    /// Optional: encrypted metadata (device info, version, etc.)
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterClientResponse {
    pub client_id: Uuid,
    pub session_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AuthenticateClientRequest {
    pub client_identifier: String,

    /// Optional: signed challenge for enhanced security
    pub challenge_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateClientResponse {
    pub client_id: Uuid,
    pub session_token: String,
    pub expires_at: DateTime<Utc>,
    pub is_new_session: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationChallenge {
    pub challenge: String,
    pub expires_at: DateTime<Utc>,
}

// Cache key for looking up a client by session token hash
pub fn cache_client_session_key(session_hash: &str) -> String {
    cache_key!("client", "session", session_hash)
}

// Cache key for looking up a client by their public identifier (like short ID)
pub fn cache_client_identifier_key(identifier: &str) -> String {
    cache_key!("client", "identifier", identifier)
}

// =============================================================================
// Implementation
// =============================================================================

impl Client {
    /// Register new client (idempotent via unique hash)
    pub async fn register<'c, E>(
        exec: E,
        input: RegisterClientRequest,
        developer_id: Option<Uuid>,
        api_key_id: Option<Uuid>,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres>,
    {
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        let client_identifier_hash = crypto::hash_content(input.client_identifier.as_bytes());
        let token = crypto::generate_secure_token::<32>()?;
        let session_token = BASE64.encode(&token);
        let session_token_hash = crypto::hash_content(&token);
        let session_expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS);

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
            api_key_id,
            last_seen_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
        RETURNING *
        "#,
        )
        .bind(&input.identifier)
        .bind(&client_identifier_hash)
        .bind(&input.public_key)
        .bind(&session_token_hash)
        .bind(session_expires_at)
        .bind(&input.metadata)
        .bind(developer_id)
        .bind(api_key_id)
        .fetch_one(exec)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Client already registered".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at: session_expires_at,
        })
    }

    /// Authenticate client by hashed identifier
    pub async fn authenticate<'c, E>(
        exec: E,
        input: AuthenticateClientRequest,
    ) -> Result<AuthenticateClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        let client_identifier_hash = crypto::hash_content(input.client_identifier.as_bytes());

        let client = sqlx::query_as::<_, Client>(
            r#"SELECT * FROM clients WHERE client_identifier_hash = $1"#,
        )
        .bind(&client_identifier_hash)
        .fetch_optional(exec.clone())
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Client not found".to_string()))?;

        if !client.is_active {
            return Err(VaultlessError::Unauthorized(
                "Client is deactivated".to_string(),
            ));
        }

        let is_new_session = client
            .session_expires_at
            .map_or(true, |exp| exp < Utc::now());

        let (session_token, expires_at) = if is_new_session {
            let token = crypto::generate_secure_token::<32>()?;
            let session_token = BASE64.encode(&token);
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

            (session_token, expires_at)
        } else {
            sqlx::query("UPDATE clients SET last_seen_at = NOW() WHERE id = $1")
                .bind(client.id)
                .execute(exec)
                .await?;

            (String::new(), client.session_expires_at.unwrap())
        };

        Ok(AuthenticateClientResponse {
            client_id: client.id,
            session_token,
            expires_at,
            is_new_session,
        })
    }

    /// Verify session token validity
    pub async fn verify_session<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        session_token: &str,
    ) -> Result<ClientPublic>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let token_bytes = BASE64
            .decode(session_token)
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;
        let session_token_hash = crypto::hash_content(&token_bytes);
        let cache_key = cache_client_session_key(&session_token_hash);

        // --- 1. Redis Cache Lookup ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(public_client) = serde_json::from_str::<ClientPublic>(&cached_json) {
                        tracing::debug!("Cache hit for client session");
                        return Ok(public_client);
                    }
                }
            }
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

        let public_client = ClientPublic::from(&client);

        // --- 3. Write-Back to Redis ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Some(expiry) = client.session_expires_at {
                    let ttl_secs = (expiry - Utc::now()).num_seconds().max(0);
                    if ttl_secs > 0 {
                        let serialized = serde_json::to_string(&public_client)?;
                        if let Err(e) = conn
                            .set_ex::<_, _, ()>(&cache_key, serialized, ttl_secs as u64)
                            .await
                        {
                            tracing::warn!("Redis cache write failed: {}", e);
                        }
                    }
                }
            }
        }

        Ok(public_client)
    }

    /// Lookup client by plaintext identifier (hashed internally)
    pub async fn find_by_identifier<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        client_identifier: &str,
    ) -> Result<Option<ClientPublic>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let hash = crypto::hash_content(client_identifier.as_bytes());
        let cache_key = cache_client_identifier_key(client_identifier);

        // --- 1. Redis Cache Lookup ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(public_client) = serde_json::from_str::<ClientPublic>(&cached_json) {
                        tracing::debug!("Cache hit for client identifier: {}", client_identifier);
                        return Ok(Some(public_client));
                    }
                }
            }
        }

        // --- 2. Database Lookup ---
        let client = sqlx::query_as::<_, Client>(
            r#"SELECT * FROM clients WHERE client_identifier_hash = $1"#,
        )
        .bind(&hash)
        .fetch_optional(exec)
        .await?;

        // --- 3. Write-Back to Redis ---
        if let (Some(redis_pool), Some(client)) = (&redis, &client) {
            if let Ok(mut conn) = redis_pool.get().await {
                if client.is_active {
                    if let Ok(serialized) = serde_json::to_string(&ClientPublic::from(client)) {
                        if let Err(e) = conn
                            .set_ex::<_, _, ()>(&cache_key, serialized, IDENTIFIER_TTL_SECS)
                            .await
                        {
                            tracing::warn!("Redis cache write failed for {}: {:?}", cache_key, e);
                        }
                    }
                }
            }
        }

        Ok(client.map(ClientPublic::from))
    }

    /// Lookup client by their public short identifier (non-secret, shareable)
    pub async fn find_by_public_identifier<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        identifier: &str,
    ) -> Result<Option<ClientPublic>>
    where
        E: Executor<'c, Database = Postgres>,
    {
        let cache_key = cache_client_identifier_key(identifier);

        // --- 1. Redis Cache Lookup ---
        if let Some(redis_pool) = &redis {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(cached_json) = conn.get::<_, String>(&cache_key).await {
                    if let Ok(public_client) = serde_json::from_str::<ClientPublic>(&cached_json) {
                        tracing::debug!("Cache hit for public identifier: {}", identifier);
                        return Ok(Some(public_client));
                    }
                }
            }
        }

        // --- 2. Database Lookup ---
        let client = sqlx::query_as::<_, Client>(
            r#"SELECT * FROM clients WHERE identifier = $1 AND is_active = TRUE"#,
        )
        .bind(identifier)
        .fetch_optional(exec)
        .await?;

        // --- 3. Write-Back to Redis ---
        if let (Some(redis_pool), Some(client)) = (&redis, &client) {
            if let Ok(mut conn) = redis_pool.get().await {
                if let Ok(serialized) = serde_json::to_string(&ClientPublic::from(client)) {
                    if let Err(e) = conn
                        .set_ex::<_, _, ()>(&cache_key, serialized, IDENTIFIER_TTL_SECS)
                        .await
                    {
                        tracing::warn!("Redis cache write failed for {}: {:?}", cache_key, e);
                    }
                }
            }
        }

        Ok(client.map(ClientPublic::from))
    }

    /// Mark client as recently active
    pub async fn update_last_seen<'c, E>(exec: E, client_id: Uuid) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query("UPDATE clients SET last_seen_at = NOW() WHERE id = $1")
            .bind(client_id)
            .execute(exec)
            .await?;
        Ok(())
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
        identifier: Option<&str>,
    ) -> Result<()>
    where
        E: Executor<'c, Database = Postgres>,
    {
        sqlx::query("UPDATE clients SET is_active = FALSE WHERE id = $1")
            .bind(client_id)
            .execute(exec)
            .await?;

        // --- Invalidate Redis Cache ---
        if let (Some(redis), Some(identifier)) = (redis, identifier) {
            let cache_key = cache_client_identifier_key(identifier);
            if let Ok(mut conn) = redis.get().await {
                let _ = conn.del::<_, ()>(&cache_key).await.ok();
                tracing::debug!("Deactivated client cache for identifier {}", identifier);
            }
        }

        Ok(())
    }

    /// Generate a temporary authentication challenge (5 min)
    pub fn generate_challenge() -> Result<AuthenticationChallenge> {
        let raw_bytes: [u8; 32] = crypto::generate_secure_token::<32>()?;
        Ok(AuthenticationChallenge {
            challenge: BASE64.encode(raw_bytes),
            expires_at: Utc::now() + Duration::seconds(CHALLENGE_EXPIRY_SECONDS),
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

        if let Ok(mut conn) = redis.get().await {
            if conn.del::<_, ()>(&cache_key).await.is_ok() {
                tracing::debug!("Invalidated session cache: {}", cache_key);
            }
        }

        Ok(())
    }
    /// Invalidate cached client identifier (after identifier update or deactivation)
    pub async fn invalidate_client_identifier_cache(
        redis: &Arc<RedisPool>,
        identifier: &str,
    ) -> Result<()> {
        let cache_key = cache_client_identifier_key(identifier);

        if let Ok(mut conn) = redis.get().await {
            if conn.del::<_, ()>(&cache_key).await.is_ok() {
                tracing::debug!("Invalidated identifier cache: {}", cache_key);
            }
        }

        Ok(())
    }
}

impl From<&Client> for ClientPublic {
    fn from(c: &Client) -> Self {
        Self {
            id: c.id,
            identifier: c.identifier.clone(),
            public_key: c.public_key.clone(),
            allow_anonymous_messages: c.allow_anonymous_messages,
            require_proof_verification: c.require_proof_verification,
            is_active: c.is_active,
            last_seen_at: c.last_seen_at,
            last_message_at: c.last_message_at,
        }
    }
}
