use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use vaultless_core::getrandom;

use crate::middleware::error::ApiError;
use crate::services::cache::CacheService;

/// Token pair (access + refresh)
#[derive(Debug)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

/// Session data stored in Dragonfly
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub user_id: String,
    pub email: String,
    pub scope: Option<String>,
    pub is_admin: bool,
    pub created_at: i64,
}

/// Refresh token data stored in Dragonfly
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefreshTokenCache {
    user_id: String,
    token_family: String,
    is_used: bool,
    is_revoked: bool,
    expires_at: i64,
}

/// Token service for OAuth-like authentication
pub struct TokenService {
    db: PgPool,
    cache: CacheService,
}

impl TokenService {
    pub fn new(db: PgPool, cache_pool: RedisPool) -> Self {
        let cache = CacheService::new(cache_pool, 3600); // 1 hour default TTL
        Self { db, cache }
    }

    /// Generate cryptographically secure random token
    fn generate_token() -> Result<String, getrandom::Error> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)?;
        Ok(URL_SAFE_NO_PAD.encode(seed))
    }

    /// Hash token for storage (SHA-256)
    fn hash_token(token: &str) -> String {
        vaultless_core::crypto::hash_content(token.as_bytes())
    }

    /// Generate session cache key
    fn session_key(token_hash: &str) -> String {
        format!("session:{}", token_hash)
    }

    /// Generate refresh token cache key
    fn refresh_key(token_hash: &str) -> String {
        format!("refresh:{}", token_hash)
    }

    /// Generate login failure cache key
    fn login_fail_key(ip: &str) -> String {
        format!("login_fail:{}", ip)
    }

    /// Create access + refresh token pair
    pub async fn create_token_pair(
        &self,
        user_id: Uuid,
        email: String,
        scope: Option<String>,
        is_admin: bool,
    ) -> Result<TokenPair, ApiError> {
        // Generate access token
        let access_token = Self::generate_token().map_err(|e| {
            tracing::error!("Failed to generate access token: {}", e);
            ApiError::internal_server_error(format!("Failed to generate access token: {}", e))
        })?;

        // Generate refresh token
        let refresh_token = Self::generate_token().map_err(|e| {
            tracing::error!("Failed to generate refresh token: {}", e);
            ApiError::internal_server_error(format!("Failed to generate refresh token: {}", e))
        })?;

        let token_family = Uuid::new_v4();

        let access_token_hash = Self::hash_token(&access_token);
        let refresh_token_hash = Self::hash_token(&refresh_token);

        let access_ttl: i64 = 3600; // 1 hour
        let refresh_ttl_days: i64 = 30; // 30 days
        let now = Utc::now().timestamp();

        // 1. Store session in Dragonfly (hot path)
        let session_data = SessionData {
            user_id: user_id.to_string(),
            email: email.clone(),
            scope: scope.clone(),
            is_admin,
            created_at: now,
        };

        self.cache
            .set_with_ttl(
                &Self::session_key(&access_token_hash),
                &session_data,
                std::time::Duration::from_secs(access_ttl as u64),
            )
            .await?;

        tracing::debug!("Session cached in Dragonfly: {}", access_token_hash);

        // 2. Store refresh token in Dragonfly
        let refresh_cache = RefreshTokenCache {
            user_id: user_id.to_string(),
            token_family: token_family.to_string(),
            is_used: false,
            is_revoked: false,
            expires_at: now + (refresh_ttl_days * 86400),
        };

        self.cache
            .set_with_ttl(
                &Self::refresh_key(&refresh_token_hash),
                &refresh_cache,
                std::time::Duration::from_secs((refresh_ttl_days * 86400) as u64), // 30 days
            )
            .await?;

        tracing::debug!("Refresh token cached in Dragonfly: {}", refresh_token_hash);

        // 3. Log session in Postgres (audit trail) - async, non-blocking
        let db = self.db.clone();
        let scope_clone = scope.clone();
        tokio::spawn(async move {
            if let Err(e) = vaultless_core::models::auth::UserSession::create(
                &db,
                user_id,
                access_token_hash,
                scope_clone,
                access_ttl,
            )
            .await
            {
                tracing::warn!("Failed to log session in Postgres: {}", e);
            }
        });

        // 4. Store refresh token in Postgres (audit + fallback)
        let db_clone = self.db.clone();
        tokio::spawn(async move {
            if let Err(e) = vaultless_core::models::auth::RefreshToken::create(
                &db_clone,
                user_id,
                refresh_token_hash,
                token_family,
                refresh_ttl_days,
            )
            .await
            {
                tracing::warn!("Failed to store refresh token in Postgres: {}", e);
            }
        });

        Ok(TokenPair {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: access_ttl,
        })
    }

    /// Verify and get user from access token (Dragonfly first)
    pub async fn verify_access_token(&self, token: &str) -> Result<SessionData, ApiError> {
        let token_hash = Self::hash_token(token);
        let cache_key = Self::session_key(&token_hash);

        // Fast path: Check Dragonfly
        match self.cache.get::<SessionData>(&cache_key).await? {
            Some(session) => {
                tracing::debug!("Session cache hit for {}", token_hash);
                return Ok(session);
            }
            None => {
                tracing::debug!("Session cache miss for {}", token_hash);
            }
        }

        // Fallback: Check Postgres and repopulate cache
        let session_db =
            vaultless_core::models::auth::UserSession::find_by_token_hash(&self.db, &token_hash)
                .await
                .map_err(|_| ApiError::unauthorized("Invalid or expired token"))?;

        let user = vaultless_core::models::auth::User::find_by_id(&self.db, session_db.user_id)
            .await
            .map_err(ApiError::from)?;

        if !user.is_active {
            return Err(ApiError::forbidden("User account is deactivated"));
        }

        // Repopulate cache
        let session_data = SessionData {
            user_id: user.id.to_string(),
            email: user.email,
            scope: session_db.scope,
            is_admin: user.is_admin,
            created_at: session_db.created_at.timestamp(),
        };

        let remaining_ttl = (session_db.expires_at - Utc::now()).num_seconds().max(0);

        if remaining_ttl > 0 {
            if let Err(e) = self
                .cache
                .set_with_ttl(
                    &cache_key,
                    &session_data,
                    std::time::Duration::from_secs(remaining_ttl as u64),
                )
                .await
            {
                tracing::warn!("Failed to repopulate session cache: {}", e);
            } else {
                tracing::debug!("Session cache repopulated from Postgres");
            }
        }

        Ok(session_data)
    }

    /// Refresh access token using refresh token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenPair, ApiError> {
        let refresh_token_hash = Self::hash_token(refresh_token);
        let cache_key = Self::refresh_key(&refresh_token_hash);

        // Check Dragonfly first
        let cached_token = self.cache.get::<RefreshTokenCache>(&cache_key).await?;

        let (user_id, token_family, is_used, is_revoked) = if let Some(cache_data) = cached_token {
            tracing::debug!("Refresh token cache hit");

            // Parse user_id from cache
            let user_id = Uuid::parse_str(&cache_data.user_id)
                .map_err(|_| ApiError::internal_server_error("Invalid user ID in cached token"))?;

            let token_family = Uuid::parse_str(&cache_data.token_family).map_err(|_| {
                ApiError::internal_server_error("Invalid token family in cached token")
            })?;

            (
                user_id,
                token_family,
                cache_data.is_used,
                cache_data.is_revoked,
            )
        } else {
            // Fallback to Postgres
            tracing::debug!("Refresh token cache miss, checking Postgres");

            let token = vaultless_core::models::auth::RefreshToken::find_by_hash(
                &self.db,
                &refresh_token_hash,
            )
            .await
            .map_err(|_| ApiError::unauthorized("Invalid refresh token"))?;

            (
                token.user_id,
                token.token_family,
                token.is_used,
                token.is_revoked,
            )
        };

        // Check if already used (theft detection)
        if is_used {
            tracing::warn!(
                user_id = %user_id,
                token_family = %token_family,
                "Refresh token reuse detected - revoking family"
            );

            // Revoke entire family
            vaultless_core::models::auth::RefreshToken::revoke_family(&self.db, token_family)
                .await
                .map_err(ApiError::from)?;

            // Remove from cache
            self.cache.delete(&cache_key).await?;

            return Err(ApiError::forbidden(
                "Token reuse detected - all tokens revoked",
            ));
        }

        if is_revoked {
            return Err(ApiError::unauthorized("Refresh token has been revoked"));
        }

        // Get user info
        let user = vaultless_core::models::auth::User::find_by_id(&self.db, user_id)
            .await
            .map_err(ApiError::from)?;

        // Generate new tokens
        let new_access_token = Self::generate_token().map_err(|e| {
            tracing::error!("Failed to generate new access token: {}", e);
            ApiError::internal_server_error(format!("Failed to generate access token: {}", e))
        })?;

        let new_refresh_token = Self::generate_token().map_err(|e| {
            tracing::error!("Failed to generate new refresh token: {}", e);
            ApiError::internal_server_error(format!("Failed to generate refresh token: {}", e))
        })?;

        let new_access_hash = Self::hash_token(&new_access_token);
        let new_refresh_hash = Self::hash_token(&new_refresh_token);

        // Mark old refresh token as used in cache
        let updated_cache = RefreshTokenCache {
            user_id: user_id.to_string(),
            token_family: token_family.to_string(),
            is_used: true,
            is_revoked: false,
            expires_at: Utc::now().timestamp() + (30 * 86400),
        };

        // Update in cache (mark as used)
        self.cache
            .set_with_ttl(
                &cache_key,
                &updated_cache,
                std::time::Duration::from_secs(60), // Short TTL for used tokens
            )
            .await?;

        // Rotate in Postgres (async)
        let db = self.db.clone();
        let old_hash = refresh_token_hash.clone();
        let new_hash_clone = new_refresh_hash.clone();
        tokio::spawn(async move {
            // Find old token first
            if let Ok(old_token) =
                vaultless_core::models::auth::RefreshToken::find_by_hash(&db, &old_hash).await
                && let Err(e) = vaultless_core::models::auth::RefreshToken::rotate(
                    &db,
                    old_token.id,
                    new_hash_clone,
                )
                .await
            {
                tracing::warn!("Failed to rotate refresh token in Postgres: {}", e);
            }
        });

        // Store new refresh token in cache
        let new_refresh_cache = RefreshTokenCache {
            user_id: user_id.to_string(),
            token_family: token_family.to_string(),
            is_used: false,
            is_revoked: false,
            expires_at: Utc::now().timestamp() + (30 * 86400),
        };

        self.cache
            .set_with_ttl(
                &Self::refresh_key(&new_refresh_hash),
                &new_refresh_cache,
                std::time::Duration::from_secs(30 * 86400),
            )
            .await?;

        // Store new access token session in cache
        let access_ttl = 3600;
        let now = Utc::now().timestamp();

        let session_data = SessionData {
            user_id: user.id.to_string(),
            email: user.email.clone(),
            scope: None,
            is_admin: user.is_admin,
            created_at: now,
        };

        self.cache
            .set_with_ttl(
                &Self::session_key(&new_access_hash),
                &session_data,
                std::time::Duration::from_secs(access_ttl as u64),
            )
            .await?;

        // Log in Postgres (async)
        let db = self.db.clone();
        tokio::spawn(async move {
            if let Err(e) = vaultless_core::models::auth::UserSession::create(
                &db,
                user.id,
                new_access_hash,
                None,
                access_ttl,
            )
            .await
            {
                tracing::warn!("Failed to log refreshed session: {}", e);
            }
        });

        Ok(TokenPair {
            access_token: new_access_token,
            refresh_token: new_refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: access_ttl,
        })
    }

    /// Revoke access token (remove from cache immediately)
    pub async fn revoke_access_token(&self, token: &str) -> Result<(), ApiError> {
        let token_hash = Self::hash_token(token);
        let cache_key = Self::session_key(&token_hash);

        // Remove from cache immediately
        self.cache.delete(&cache_key).await?;

        tracing::info!("Access token revoked from cache: {}", token_hash);

        // Also mark as revoked in Postgres (audit trail)
        let db = self.db.clone();
        tokio::spawn(async move {
            if let Ok(session) =
                vaultless_core::models::auth::UserSession::find_by_token_hash(&db, &token_hash)
                    .await
                && let Err(e) =
                    vaultless_core::models::auth::UserSession::revoke(&db, session.id).await
            {
                tracing::warn!("Failed to revoke session in Postgres: {}", e);
            }
        });

        Ok(())
    }

    /// Revoke all user sessions (logout everywhere)
    pub async fn revoke_all_user_tokens(&self, user_id: Uuid) -> Result<(), ApiError> {
        // Note: We can't efficiently scan all session keys in Redis
        // So we rely on PostgreSQL to list sessions, then delete from cache

        // Revoke in Postgres first
        vaultless_core::models::auth::UserSession::revoke_all_for_user(&self.db, user_id)
            .await
            .map_err(ApiError::from)?;

        tracing::info!("All sessions revoked for user: {}", user_id);

        // Note: Sessions will naturally expire from cache (max 1 hour)
        // For immediate effect, we'd need to track user -> session mappings in Redis

        Ok(())
    }

    /// Track login failure for rate limiting
    pub async fn track_login_failure(&self, ip: &str) -> Result<i64, ApiError> {
        let key = Self::login_fail_key(ip);

        // Increment counter
        let count = self.cache.incr(&key).await?;

        // Set TTL on first failure
        if count == 1 {
            self.cache
                .expire(&key, std::time::Duration::from_secs(15 * 60)) // 15 minutes
                .await?;
        }

        tracing::debug!("Login failures for {}: {}", ip, count);

        Ok(count)
    }

    /// Check if IP is rate limited (5 failures in 15 min)
    pub async fn is_rate_limited(&self, ip: &str) -> Result<bool, ApiError> {
        let key = Self::login_fail_key(ip);

        match self.cache.get::<i64>(&key).await? {
            Some(count) => Ok(count >= 5),
            None => Ok(false),
        }
    }

    /// Clear login failures (after successful login)
    pub async fn clear_login_failures(&self, ip: &str) -> Result<(), ApiError> {
        let key = Self::login_fail_key(ip);
        self.cache.delete(&key).await?;
        Ok(())
    }
}
