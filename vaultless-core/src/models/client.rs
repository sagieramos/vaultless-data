use crate::cache_key;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration, Utc};
use deadpool_redis::Pool as RedisPool;
use redis::{AsyncCommands, Script};
use serde::{Deserialize, Serialize};
use sqlx::{Executor, FromRow, Postgres};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

use crate::{
    crypto,
    error::{Result, VaultlessError},
    models::app_model::dto::Application,
};

// =============================================================================
// Constants
// =============================================================================

const SESSION_DURATION_HOURS: i64 = 24 * 30; // 30 days
const CHALLENGE_EXPIRY_SECONDS: u64 = 300; // 5 minutes
const IDENTIFIER_TTL_SECS: u64 = 600; // 10 minutes

// =============================================================================
// Models
// =============================================================================

pub const MINIMAL_CLIENT_FIELDS: &str = "
    id,
    identifier,
    client_identifier_hash,
    public_key,
    session_token_hash,
    session_expires_at,
    allow_anonymous_messages,
    require_proof_verification,
    is_active,
    created_at,
    updated_at,
    last_seen_at,
    last_message_at,
    developer_id,
    api_key_id,
    metadata
";

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Client {
    #[serde(skip_serializing)]
    pub id: Uuid,
    pub identifier: Option<String>,
    #[serde(skip_serializing)]
    pub client_identifier_hash: Option<String>,
    pub public_key: Option<String>,
    #[serde(skip_serializing)]
    pub session_token_hash: Option<String>,
    #[serde(skip_serializing)]
    pub session_expires_at: Option<DateTime<Utc>>,
    pub allow_anonymous_messages: bool,
    pub require_proof_verification: bool,
    pub is_active: bool,
    #[serde(skip_serializing)]
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing)]
    pub updated_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_message_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing)]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing)]
    pub developer_id: Option<Uuid>,
    #[serde(skip_serializing)]
    pub api_key_id: Option<Uuid>,
    #[serde(skip_serializing)]
    pub application_id: Option<Uuid>, // NEW: Links client to the application they registered through
}

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

    /// Optional: UTC timestamp seconds for freshness checks (optional but recommended)
    pub timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterClientResponse {
    #[serde(skip_serializing)]
    pub client_id: Uuid,
    pub session_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AuthenticateClientRequest {
    pub client_identifier_hash: Option<String>,
    pub identifier: Option<String>,
    pub public_key: Option<String>,

    /// The original Base64-encoded challenge string received from the server.
    #[validate(length(min = 32))]
    pub challenge: String,

    /// The Base64-encoded signature of the `challenge`.
    #[validate(length(min = 32))]
    pub challenge_signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateClientResponse {
    #[serde(skip_serializing)]
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

// Cache key for storing a temporary authentication challenge
pub fn cache_auth_challenge_key(challenge_hash: &str) -> String {
    cache_key!("auth_challenge", challenge_hash)
}

// =============================================================================
// Implementation
// =============================================================================

impl Client {
    /// Register new client with application support (idempotent via unique hash)
    pub async fn register<'c, E>(
        exec: E,
        redis: Option<Arc<RedisPool>>,
        input: RegisterClientRequest,
        publishable_key: String,
    ) -> Result<RegisterClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
        // --- Step 1: Look up application by publishable key ---
        let app =
            Application::find_by_publishable_key(exec.clone(), redis.clone(), &publishable_key)
                .await
                .map_err(|e| {
                    tracing::warn!("Invalid publishable key: {}", publishable_key);
                    VaultlessError::Unauthorized(format!("Invalid publishable key: {}", e))
                })?;

        // Verify application is active
        if !app.is_active {
            return Err(VaultlessError::Unauthorized(
                "Application is deactivated".into(),
            ));
        }

        tracing::debug!(
            application_id = %app.id,
            application_name = %app.name,
            developer_id = %app.user_id,
            "Client registering through application"
        );

        // --- Step 2: Validate basic struct constraints ---
        input
            .validate()
            .map_err(|e| VaultlessError::Validation(e.to_string()))?;

        // --- Step 3: Enforce mandatory fields ---
        let pubkey = input
            .public_key
            .ok_or_else(|| VaultlessError::Validation("public_key is required.".to_string()))?;

        let signature = input
            .signature
            .ok_or_else(|| VaultlessError::Validation("signature is required.".to_string()))?;

        let payload = input
            .signed_payload
            .ok_or_else(|| VaultlessError::Validation("signed_payload is required.".to_string()))?;

        // --- Step 4: Signature Verification (Mandatory) ---
        tracing::debug!("Verifying registration signature...");
        match crate::crypto::verify_signature(payload.as_bytes(), &signature, &pubkey) {
            Ok(()) => tracing::debug!("✅ Signature verification passed"),
            Err(e) => {
                tracing::warn!("❌ Signature verification failed: {:?}", e);
                return Err(VaultlessError::Validation(
                    "Signature verification failed".into(),
                ));
            }
        }

        // --- Step 5: Nonce replay protection ---
        if let (Some(nonce), Some(redis_pool)) = (input.nonce.as_ref(), &redis) {
            let nonce_key = cache_key!("client", "register_nonce", nonce);

            if let Ok(mut conn) = redis_pool.get().await {
                const NONCE_SCRIPT: &str = r#"
                local key = KEYS[1]
                local ttl = tonumber(ARGV[1])
                local ok = redis.call('SET', key, '1', 'NX', 'EX', ttl)
                if ok then
                    return 1
                else
                    return 0
                end
            "#;
                let script = Script::new(NONCE_SCRIPT);
                let script_result: redis::RedisResult<i32> = script
                    .key(&nonce_key)
                    .arg(IDENTIFIER_TTL_SECS)
                    .invoke_async(&mut conn)
                    .await;

                match script_result {
                    Ok(1) => {
                        tracing::debug!("Nonce reserved in redis for key {}", nonce_key);
                    }
                    Ok(0) => {
                        return Err(VaultlessError::Validation("Nonce already used".into()));
                    }
                    Ok(other) => {
                        tracing::warn!(
                            "Unexpected redis script result for nonce key {}: {}",
                            nonce_key,
                            other
                        );
                        return Err(VaultlessError::Validation(
                            "Nonce check failed due to unexpected redis response".into(),
                        ));
                    }
                    Err(e) => {
                        tracing::warn!("Redis error while checking nonce with script: {}", e);
                    }
                }
            } else {
                tracing::warn!(
                    "Could not acquire redis connection for nonce check; continuing without replay protection"
                );
            }
        }

        // --- Step 6: Timestamp freshness check ---
        if let Some(ts) = input.timestamp {
            let now_unix = Utc::now().timestamp();
            let skew_allowed = 300; // 5 minutes allowed drift
            if (ts - now_unix).abs() > skew_allowed {
                return Err(VaultlessError::Validation(
                    "Timestamp is outside allowed time window".into(),
                ));
            }
        }

        // --- Step 7: Compute client_identifier hash ---
        let client_identifier_hash = input
            .client_identifier
            .as_ref()
            .map(|ci| crypto::hash_content(ci.as_bytes()));

        // --- Step 8: Generate session token ---
        let token = crypto::generate_secure_token::<32>()?;
        let session_token = BASE64.encode(token);
        let session_token_hash = crypto::hash_content(&token);
        let session_expires_at = Utc::now() + Duration::hours(SESSION_DURATION_HOURS);

        // --- Step 9: Insert into database with application references ---
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
                application_id,
                last_seen_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
            RETURNING *
            "#,
        )
        .bind(&input.identifier)
        .bind(&client_identifier_hash)
        .bind(&pubkey)
        .bind(&session_token_hash)
        .bind(session_expires_at)
        .bind(&input.metadata)
        .bind(app.user_id) // developer_id from application
        .bind(app.secret_key_id) // api_key_id for billing/metrics
        .bind(app.id) // application_id (NEW)
        .fetch_one(exec)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Duplicate("Client already registered".to_string())
            }
            _ => VaultlessError::Database(e),
        })?;

        tracing::info!(
            client_id = %client.id,
            application_id = %app.id,
            developer_id = %app.user_id,
            "Client registered successfully"
        );

        // --- Step 10: Cache canonical and aliases in Redis ---
        if let Some(redis_pool) = &redis {
            let _ = Self::cache_to_redis(redis_pool, &client).await;
        }

        Ok(RegisterClientResponse {
            client_id: client.id,
            session_token,
            expires_at: session_expires_at,
        })
    }

    /// Authenticate client by hashed identifier
    pub async fn authenticate<'c, E>(
        exec: E,
        redis: Arc<RedisPool>,
        input: AuthenticateClientRequest,
    ) -> Result<AuthenticateClientResponse>
    where
        E: Executor<'c, Database = Postgres> + Clone,
    {
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

        // --- 5. Verify the signed challenge ---
        // We verify the *original challenge string* against the signature.
        if !client.verify_signature(&input.challenge, &input.challenge_signature)? {
            return Err(VaultlessError::Unauthorized(
                "Invalid challenge signature".into(),
            ));
        }

        // --- 6. Generate new session token (always fresh, no expiry check) ---
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
        })
    }

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

    async fn cache_to_redis(redis_pool: &Arc<RedisPool>, client: &Client) -> Result<()> {
        if let Ok(mut conn) = redis_pool.get().await {
            let ttl_secs = 24 * 60 * 60; // 24 hours

            let serialized = serde_json::to_string(&client)
                .map_err(|e| VaultlessError::Serialization(e.to_string()))?;

            let mut pipe = redis::pipe();
            let client_id_key = cache_key!("client", "id", client.id);

            // Canonical client cache
            pipe.cmd("SETEX")
                .arg(&client_id_key)
                .arg(ttl_secs)
                .arg(&serialized);

            // Alias: public_key
            if let Some(ref pk) = client.public_key {
                pipe.cmd("SETEX")
                    .arg(cache_key!("client", "alias", "public_key", pk))
                    .arg(ttl_secs)
                    .arg(client.id.to_string());
            }

            // Alias: identifier
            if let Some(ref idf) = client.identifier {
                pipe.cmd("SETEX")
                    .arg(cache_key!("client", "alias", "identifier", idf))
                    .arg(ttl_secs)
                    .arg(client.id.to_string());
            }

            // Alias: client_identifier_hash
            if let Some(ref cid_hash_value) = client.client_identifier_hash {
                pipe.cmd("SETEX")
                    .arg(cache_key!(
                        "client",
                        "alias",
                        "client_identifier_hash",
                        cid_hash_value
                    ))
                    .arg(ttl_secs)
                    .arg(client.id.to_string());
            }

            // Execute the pipeline atomically
            if let Err(e) = pipe.query_async::<()>(&mut conn).await {
                tracing::warn!("Redis pipeline write failed: {}", e);
            } else {
                tracing::debug!("Client cached successfully in Redis");
            }
        }
        Ok(())
    }
}
