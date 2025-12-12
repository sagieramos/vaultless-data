// vaultless-core/src/models/session/paseto_session.rs

use super::claims_keys as ck;
use crate::cache_key;
use crate::error::{Result, VaultlessError};
use chrono::{Duration, Utc};
use pasetors::claims::{Claims, ClaimsValidationRules};
use pasetors::keys::Generate;
use pasetors::keys::SymmetricKey;
use pasetors::token::{Local, UntrustedToken};
use pasetors::{local, version4::V4};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// =============================================================================
// SESSION TOKEN KEY MANAGEMENT
// =============================================================================

/// Thread-safe session token key manager with rotation support
pub struct SessionKeyManager {
    current_key: SymmetricKey<V4>,
    previous_key: Option<SymmetricKey<V4>>,
}

impl fmt::Debug for SessionKeyManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionKeyManager")
            .field("current_key", &"<redacted>") // Hide the key bytes
            .field("previous_key", &self.previous_key.is_some()) // Bool: true/false if present
            .finish()
    }
}

impl SessionKeyManager {
    /// Create new key manager from hex-encoded keys
    pub fn new(current_key_hex: &str, previous_key_hex: Option<&str>) -> Result<Self> {
        let current_key = SymmetricKey::<V4>::from(
            &hex::decode(current_key_hex)
                .map_err(|e| VaultlessError::Internal(format!("Invalid current key hex: {e}")))?,
        )
        .map_err(|e| VaultlessError::Internal(format!("Invalid current key: {e}")))?;

        let previous_key = previous_key_hex
            .map(|prev_hex| {
                SymmetricKey::<V4>::from(&hex::decode(prev_hex).map_err(|e| {
                    VaultlessError::Internal(format!("Invalid previous key hex: {e}"))
                })?)
                .map_err(|e| VaultlessError::Internal(format!("Invalid previous key: {e}")))
            })
            .transpose()?;

        Ok(Self {
            current_key,
            previous_key,
        })
    }

    /// Generate a new random key (for initialization or rotation)
    pub fn generate_new_key() -> String {
        let key = SymmetricKey::<V4>::generate().expect("Failed to generate key");
        hex::encode(key.as_bytes())
    }

    /// Get current key for signing
    pub fn current(&self) -> &SymmetricKey<V4> {
        &self.current_key
    }

    /// Try current key first, then previous (key rotation support)
    pub fn verify_with_rotation(&self, token: &str) -> Result<Claims> {
        self.verify_token_with_key(token, &self.current_key)
            .or_else(|_| {
                if let Some(prev_key) = &self.previous_key {
                    self.verify_token_with_key(token, prev_key)
                } else {
                    Err(VaultlessError::Unauthorized(
                        "Invalid or expired token".into(),
                    ))
                }
            })
    }

    /// Internal: verify using a specific key
    fn verify_token_with_key(&self, token: &str, key: &SymmetricKey<V4>) -> Result<Claims> {
        let untrusted = UntrustedToken::<Local, V4>::try_from(token)
            .map_err(|e| VaultlessError::Unauthorized(format!("Malformed token: {e}")))?;

        let rules = ClaimsValidationRules::new();

        let trusted_token = local::decrypt(key, &untrusted, &rules, None, None)
            .map_err(|e| VaultlessError::Unauthorized(format!("Token verification failed: {e}")))?;

        let claims = trusted_token
            .payload_claims()
            .ok_or_else(|| VaultlessError::Unauthorized("Token has no claims".into()))?
            .clone();

        Ok(claims)
    }
}

// =============================================================================
// SESSION CLAIMS
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub platform: String,
    pub app_fingerprint: Uuid,
    pub device_trust_score: u8,
    pub app_tier: Option<String>,
    pub application_secret_api_key_id: Option<Uuid>,
    pub pubkey: Option<String>,
}

/// Create session token with claims
pub fn create_session_token(
    key: &SymmetricKey<V4>,
    session_data: SessionData,
    ttl_seconds: u64,
) -> Result<String> {
    let mut claims = Claims::new()?;

    let now = Utc::now();
    let exp = now + Duration::seconds(ttl_seconds as i64);

    // Standard Claims
    claims.issued_at(&now.to_rfc3339())?;
    claims.expiration(&exp.to_rfc3339())?;
    claims.subject(&session_data.client_id.to_string())?;
    claims.token_identifier(&Uuid::new_v4().to_string())?;

    // Custom Claims
    claims.add_additional(ck::APPLICATION_ID, session_data.application_id.to_string())?;
    claims.add_additional(ck::PLATFORM, session_data.platform)?;
    claims.add_additional(ck::DEVICE_TRUSTED, session_data.device_trust_score)?;
    claims.add_additional(
        ck::APP_FINGERPRINT,
        session_data.app_fingerprint.to_string(),
    )?;

    if let Some(tier) = session_data.app_tier {
        claims.add_additional(ck::APP_TIER, tier)?;
    }

    if let Some(ask_id) = session_data.application_secret_api_key_id {
        claims.add_additional(ck::APP_SECRET_KEY_ID, ask_id.to_string())?;
    }

    if let Some(pubkey) = session_data.pubkey {
        claims.add_additional(ck::PUBKEY, pubkey)?;
    }

    let token = local::encrypt(key, &claims, None, None)
        .map_err(|e| VaultlessError::Internal(format!("Failed to create token: {e}")))?;

    Ok(token)
}

// =============================================================================
// FAST VERIFICATION (HOT PATH - NO EXPIRATION PARSING)
// =============================================================================

/// Verify token and extract session data + JTI (HOT PATH OPTIMIZED)
///
/// This function skips expiration parsing for performance.
/// PASETO already validates the token isn't expired during verification.
/// Use this for high-frequency authenticated requests.
pub fn verify_session_token(
    key_manager: &SessionKeyManager,
    token: &str,
) -> Result<(SessionData, String)> {
    let claims = key_manager.verify_with_rotation(token)?;

    let client_id = claims
        .get_claim(ck::SUBJECT)
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| VaultlessError::Unauthorized("Invalid sub".into()))?;

    let jti = claims
        .get_claim(ck::TOKEN_ID)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| VaultlessError::Unauthorized("Missing jti".into()))?;

    let application_id = claims
        .get_claim(ck::APPLICATION_ID)
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| VaultlessError::Unauthorized("Invalid application_id".into()))?;

    let platform = claims
        .get_claim(ck::PLATFORM)
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| "unknown".to_string());

    let device_trust_score = claims
        .get_claim(ck::DEVICE_TRUSTED)
        .and_then(|v| v.as_u64())
        .map(|n| n as u8)
        .unwrap_or(0);

    let app_fingerprint = claims
        .get_claim(ck::APP_FINGERPRINT)
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or(Uuid::nil());

    let app_tier = claims
        .get_claim(ck::APP_TIER)
        .and_then(|v| v.as_str())
        .map(String::from);

    let application_secret_api_key_id = claims
        .get_claim(ck::APP_SECRET_KEY_ID)
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    let pubkey = claims
        .get_claim(ck::PUBKEY)
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok((
        SessionData {
            client_id,
            application_id,
            platform,
            device_trust_score,
            app_fingerprint,
            app_tier,
            application_secret_api_key_id,
            pubkey,
        },
        jti,
    ))
}

// =============================================================================
// EXPIRATION EXTRACTION (SEPARATE FUNCTION)
// =============================================================================

/// Extract expiration time from token
/// Use only when you need to return expiration to the client (e.g., login response)
pub fn extract_token_expiration(
    key_manager: &SessionKeyManager,
    token: &str,
) -> Result<chrono::DateTime<Utc>> {
    let claims = key_manager.verify_with_rotation(token)?;

    claims
        .get_claim(ck::EXPIRATION)
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<chrono::DateTime<Utc>>().ok())
        .ok_or_else(|| VaultlessError::Unauthorized("Missing or invalid exp".into()))
}

// =============================================================================
// REVOCATION WITH MOKA CACHE
// =============================================================================

use deadpool_redis::Pool as RedisPool;
use moka::future::Cache;
use redis::{AsyncCommands, Script};
use std::sync::Arc;
use std::time::Duration as StdDuration;

const REVOKED_SESSION_PREFIX: &str = "revoked_session";

static REVOKE_AND_CHECK_SCRIPT: once_cell::sync::Lazy<Script> = once_cell::sync::Lazy::new(|| {
    Script::new(
        r#"
        local key = KEYS[1]
        local value = ARGV[1]
        local ttl = tonumber(ARGV[2])

        if value ~= "" then
            -- Revoke: SETEX and return old value
            local old = redis.call("GET", key)
            redis.call("SETEX", key, ttl, value)
            return old ~= false and 1 or 0
        else
            -- Check only: return 1 if exists, 0 if not
            return redis.call("EXISTS", key)
        end
    "#,
    )
});

/// Session verifier with local caching for revocation checks
pub struct SessionVerifier {
    key_manager: Arc<SessionKeyManager>,
    redis_pool: Arc<RedisPool>,
    revocation_cache: Cache<String, bool>, // jti -> is_revoked
}

impl SessionVerifier {
    /// Create new session verifier with caching
    ///
    /// # Arguments
    /// * `cache_size` - Maximum number of JTIs to cache (default: 10,000)
    /// * `cache_ttl_seconds` - How long to cache revocation status (default: 60s)
    pub fn new(
        key_manager: Arc<SessionKeyManager>,
        redis_pool: Arc<RedisPool>,
        cache_size: u64,
        cache_ttl_seconds: u64,
    ) -> Self {
        Self {
            key_manager,
            redis_pool,
            revocation_cache: Cache::builder()
                .max_capacity(cache_size)
                .time_to_live(StdDuration::from_secs(cache_ttl_seconds))
                .build(),
        }
    }

    /// Create with default cache settings (10k entries, 60s TTL)
    pub fn with_defaults(key_manager: Arc<SessionKeyManager>, redis_pool: Arc<RedisPool>) -> Self {
        Self::new(key_manager, redis_pool, 10_000, 60)
    }

    /// Verify session with cached revocation checks (HOT PATH)
    ///
    /// This checks a local in-memory cache first, then falls back to Redis.
    /// Cache misses are populated for future requests.
    pub async fn verify_fast(&self, token: &str) -> Result<SessionData> {
        let (session_data, jti) = verify_session_token(&self.key_manager, token)?;

        // Check local cache first (nanosecond latency)
        if let Some(is_revoked) = self.revocation_cache.get(&jti).await {
            if is_revoked {
                return Err(VaultlessError::Unauthorized("Session revoked".into()));
            }
            return Ok(session_data);
        }

        // Cache miss - check Redis (millisecond latency)
        let is_revoked = self.is_session_revoked_redis(&jti).await?;

        // Populate cache
        self.revocation_cache.insert(jti, is_revoked).await;

        if is_revoked {
            return Err(VaultlessError::Unauthorized("Session revoked".into()));
        }

        Ok(session_data)
    }

    /// Verify session without revocation check (FASTEST PATH)
    ///
    /// Use for non-sensitive operations where revocation can be eventually consistent.
    /// Token expiration is still validated by PASETO.
    pub async fn verify_no_revocation_check(&self, token: &str) -> Result<SessionData> {
        let (session_data, _jti) = verify_session_token(&self.key_manager, token)?;
        Ok(session_data)
    }

    /// Revoke a session
    pub async fn revoke_session(&self, jti: &str, remaining_ttl_seconds: u64) -> Result<()> {
        let key = cache_key!(REVOKED_SESSION_PREFIX, jti);
        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {e}")))?;

        let was_already_revoked: i32 = REVOKE_AND_CHECK_SCRIPT
            .key(&key)
            .arg("1")
            .arg(remaining_ttl_seconds.max(1))
            .invoke_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Lua revoke failed: {e}")))?;

        // Invalidate cache entry
        self.revocation_cache.invalidate(jti).await;

        tracing::info!(
            jti = %jti,
            ttl = remaining_ttl_seconds,
            already_revoked = was_already_revoked == 1,
            "Session revoked"
        );

        Ok(())
    }

    /// Check if session is revoked in Redis (internal helper)
    async fn is_session_revoked_redis(&self, jti: &str) -> Result<bool> {
        let key = cache_key!(REVOKED_SESSION_PREFIX, jti);

        let mut conn = self
            .redis_pool
            .get()
            .await
            .map_err(|e| VaultlessError::Internal(format!("Redis error: {e}")))?;

        let is_revoked: i32 = REVOKE_AND_CHECK_SCRIPT
            .key(&key)
            .arg("")
            .arg(0)
            .invoke_async(&mut conn)
            .await
            .map_err(|e| VaultlessError::Internal(format!("Lua check failed: {e}")))?;

        Ok(is_revoked == 1)
    }

    pub fn key_manager(&self) -> &Arc<SessionKeyManager> {
        &self.key_manager
    }

    /// Get cache statistics (useful for monitoring)
    pub fn cache_stats(&self) -> (u64, u64) {
        (
            self.revocation_cache.entry_count(),
            self.revocation_cache.weighted_size(),
        )
    }

    /// Clear the revocation cache (useful for testing or forced refresh)
    pub async fn clear_cache(&self) {
        self.revocation_cache.invalidate_all();
        self.revocation_cache.run_pending_tasks().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let key_hex = SessionKeyManager::generate_new_key();
        assert_eq!(key_hex.len(), 64);
    }

    #[test]
    fn test_session_token_roundtrip() {
        let key_hex = SessionKeyManager::generate_new_key();
        let key_manager = SessionKeyManager::new(&key_hex, None).unwrap();

        let session_data = SessionData {
            client_id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            platform: "ios".to_string(),
            device_trust_score: 85,
            app_fingerprint: Uuid::new_v4(),
            app_tier: Some("premium".to_string()),
            application_secret_api_key_id: None,
            pubkey: None,
        };

        let token =
            create_session_token(key_manager.current(), session_data.clone(), 3600).unwrap();
        assert!(token.starts_with("v4.local."));

        let (verified_data, _jti) = verify_session_token(&key_manager, &token).unwrap();
        assert_eq!(verified_data.client_id, session_data.client_id);
        assert_eq!(verified_data.platform, session_data.platform);
    }

    #[test]
    fn test_expiration_extraction() {
        let key_hex = SessionKeyManager::generate_new_key();
        let key_manager = SessionKeyManager::new(&key_hex, None).unwrap();

        let session_data = SessionData {
            client_id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            platform: "android".to_string(),
            device_trust_score: 90,
            app_fingerprint: Uuid::new_v4(),
            app_tier: None,
            application_secret_api_key_id: None,
            pubkey: None,
        };

        let token = create_session_token(key_manager.current(), session_data, 3600).unwrap();

        let expires_at = extract_token_expiration(&key_manager, &token).unwrap();
        assert!(expires_at > Utc::now());
    }
}
