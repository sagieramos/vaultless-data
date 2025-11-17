use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::models::app_model::attestation_types::AttestationRequest;
use chrono::{DateTime, Utc};
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

// =============================================================================
// Constants
// =============================================================================

pub const SESSION_DURATION_HOURS: i64 = 24 * 30; // 30 days
pub const CHALLENGE_EXPIRY_SECONDS: u64 = 300; // 5 minutes
pub const IDENTIFIER_TTL_SECS: u64 = 600; // 10 minutes

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
    application_id,
    metadata,
    is_platform_attested
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

    pub metadata: Option<serde_json::Value>,

    #[serde(skip_serializing)]
    pub developer_id: Uuid,

    #[serde(skip_serializing)]
    pub application_id: Uuid,

    pub is_platform_attested: bool,
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<AttestationRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticateClientResponse {
    #[serde(skip_serializing)]
    pub client_id: Uuid,
    pub session_token: String,
    pub expires_at: DateTime<Utc>,
    pub is_new_session: bool,
    pub was_reattested: bool,
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

impl Client {
    pub async fn cache_to_redis(redis_pool: &Arc<RedisPool>, client: &Client) -> Result<()> {
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
