use super::claims_keys as ck;

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use chrono::{DateTime, Duration, Utc};
use pasetors::claims::{Claims, ClaimsValidationRules};
use pasetors::keys::Generate;
use pasetors::keys::SymmetricKey;
use pasetors::token::{Local, UntrustedToken};
use pasetors::{local, version4::V4};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// SESSION TOKEN KEY MANAGEMENT
// =============================================================================

/// Thread-safe session token key manager with rotation support
pub struct SessionKeyManager {
    current_key: SymmetricKey<V4>,
    previous_key: Option<SymmetricKey<V4>>,
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

        // CORRECT WAY in pasetors 5.x (2025)
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

/// Create session token with claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub client_id: Uuid,
    pub application_id: Uuid,
    pub platform: String,
    pub device_trusted: bool,
    pub app_tier: Option<String>,

    // New fields
    pub publishable_key_plaintext: Option<String>,
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
    claims.add_additional(ck::DEVICE_TRUSTED, session_data.device_trusted)?;

    if let Some(tier) = session_data.app_tier {
        claims.add_additional(ck::APP_TIER, tier)?;
    }

    if let Some(pk) = session_data.publishable_key_plaintext {
        claims.add_additional(ck::PUBLISHABLE_KEY, pk)?;
    }

    if let Some(ask_id) = session_data.application_secret_api_key_id {
        claims.add_additional(ck::APP_SECRET_KEY_ID, ask_id.to_string())?;
    }

    if let Some(pubkey) = session_data.pubkey {
        claims.add_additional(ck::PUBKEY, pubkey)?;
    }
    // --------------------------------

    let token = local::encrypt(key, &claims, None, None)
        .map_err(|e| VaultlessError::Internal(format!("Failed to create token: {e}")))?;

    Ok(token)
}

/// Verify token and extract session data + jti
pub fn verify_session_token(
    key_manager: &SessionKeyManager,
    token: &str,
) -> Result<(SessionData, String)> {
    let claims = key_manager.verify_with_rotation(token)?;

    // Standard extractions
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

    let device_trusted = claims
        .get_claim(ck::DEVICE_TRUSTED)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let app_tier = claims
        .get_claim(ck::APP_TIER)
        .and_then(|v| v.as_str())
        .map(String::from);

    // 1. Publishable Key
    let publishable_key_plaintext = claims
        .get_claim(ck::PUBLISHABLE_KEY)
        .and_then(|v| v.as_str())
        .map(String::from);

    // 2. Application Secret API Key ID (UUID)
    let application_secret_api_key_id = claims
        .get_claim(ck::APP_SECRET_KEY_ID)
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok());

    // 3. Public Key (Client's pubkey)
    let pubkey = claims
        .get_claim(ck::PUBKEY)
        .and_then(|v| v.as_str())
        .map(String::from);
    // ----------------------------------

    Ok((
        SessionData {
            client_id,
            application_id,
            platform,
            device_trusted,
            app_tier,
            publishable_key_plaintext,
            application_secret_api_key_id,
            pubkey,
        },
        jti,
    ))
}

// =============================================================================
// REVOCATION BLACKLIST
// =============================================================================

use deadpool_redis::Pool as RedisPool;
use redis::{AsyncCommands, Script};

const REVOKED_SESSION_PREFIX: &str = "revoked_session";

static REVOKE_AND_CHECK_SCRIPT: once_cell::sync::Lazy<Script> = once_cell::sync::Lazy::new(|| {
    Script::new(
        r#"
        local key = KEYS[1]
        local value = ARGV[1]  -- "1" when revoking
        local ttl = tonumber(ARGV[2])

        if value then
            -- Revoke: SETEX and return old value (nil if wasn't revoked)
            local old = redis.call("GET", key)
            redis.call("SETEX", key, ttl, value)
            return old ~= false and 1 or 0  -- 1 = was already revoked
        else
            -- Check only: return 1 if exists, 0 if not
            return redis.call("EXISTS", key)
        end
    "#,
    )
});

pub async fn revoke_session(
    redis_pool: &RedisPool,
    jti: &str,
    remaining_ttl_seconds: u64,
) -> Result<()> {
    let key = cache_key!(REVOKED_SESSION_PREFIX, jti);
    let mut conn = redis_pool
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

    tracing::info!(
        jti = %jti,
        ttl = remaining_ttl_seconds,
        already_revoked = was_already_revoked == 1,
        "Session revocation attempted"
    );
    Ok(())
}

pub async fn verify_and_check_revocation_atomic(
    key_manager: &SessionKeyManager,
    redis_pool: &RedisPool,
    token: &str,
) -> Result<SessionData> {
    let (session_data, jti) = verify_session_token(key_manager, token)?;

    let key = cache_key!(REVOKED_SESSION_PREFIX, jti);
    let mut conn = redis_pool
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

    if is_revoked == 1 {
        return Err(VaultlessError::Unauthorized(
            "Session has been revoked".into(),
        ));
    }

    Ok(session_data)
}

pub async fn is_session_revoked(redis_pool: &RedisPool, jti: &str) -> Result<bool> {
    let key = cache_key!(REVOKED_SESSION_PREFIX, jti);

    let mut conn = redis_pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis connection failed: {e}")))?;

    let revoked: Option<String> = conn
        .get(&key)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Redis GET failed: {e}")))?;

    Ok(revoked.is_some())
}

pub async fn verify_and_check_revocation(
    key_manager: &SessionKeyManager,
    redis_pool: &RedisPool,
    token: &str,
) -> Result<SessionData> {
    let (session_data, jti) = verify_session_token(key_manager, token)?;

    if is_session_revoked(redis_pool, &jti).await? {
        return Err(VaultlessError::Unauthorized(
            "Session has been revoked".into(),
        ));
    }

    Ok(session_data)
}

// =============================================================================
// TESTS (unchanged – all pass)
// =============================================================================

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
            device_trusted: true,
            app_tier: Some("premium".to_string()),
            publishable_key_plaintext: None,
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
    fn test_key_rotation() {
        let old_key_hex = SessionKeyManager::generate_new_key();
        let new_key_hex = SessionKeyManager::generate_new_key();

        let old_manager = SessionKeyManager::new(&old_key_hex, None).unwrap();
        let token = create_session_token(
            old_manager.current(),
            SessionData {
                client_id: Uuid::new_v4(),
                application_id: Uuid::new_v4(),
                platform: "android".to_string(),
                device_trusted: false,
                app_tier: None,
                publishable_key_plaintext: None,
                application_secret_api_key_id: None,
                pubkey: None,
            },
            3600,
        )
        .unwrap();

        let new_manager = SessionKeyManager::new(&new_key_hex, Some(&old_key_hex)).unwrap();
        assert!(verify_session_token(&new_manager, &token).is_ok());
    }
}
