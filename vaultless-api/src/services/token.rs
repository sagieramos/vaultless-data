use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use deadpool_redis::{Pool as RedisPool, redis::AsyncCommands};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;
use vaultless_core::getrandom;

use crate::middleware::error::ApiError;

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

/// Token service for OAuth-like authentication
pub struct TokenService {
    db: PgPool,
    cache: RedisPool,
}

impl TokenService {
    pub fn new(db: PgPool, cache: RedisPool) -> Self {
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

    /// Create access + refresh token pair
    pub async fn create_token_pair(
        &self,
        user_id: Uuid,
        email: String,
        scope: Option<String>,
        is_admin: bool,
    ) -> Result<TokenPair, ApiError> {
        // Generate access token, propagating any error
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
        let now = chrono::Utc::now().timestamp();

        // 1. Store session in Dragonfly (hot path)
        let session_data = SessionData {
            user_id: user_id.to_string(),
            email: email.clone(),
            scope: scope.clone(),
            is_admin,
            created_at: now,
        };

        self.store_session_in_cache(&access_token_hash, &session_data, access_ttl)
            .await?;

        // 2. Log session in Postgres (audit trail) - async, non-blocking
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

        // 3. Store refresh token in Postgres (not in cache - security)
        vaultless_core::models::auth::RefreshToken::create(
            &self.db,
            user_id,
            refresh_token_hash,
            token_family,
            30, // 30 days
        )
        .await
        .map_err(ApiError::from)?;

        Ok(TokenPair {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: access_ttl,
        })
    }

    /// Store session in Dragonfly
    async fn store_session_in_cache(
        &self,
        token_hash: &str,
        session: &SessionData,
        ttl_secs: i64,
    ) -> Result<(), ApiError> {
        let mut conn = self.cache.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let key = format!("session:{}", token_hash);
        let value = serde_json::to_string(session).map_err(|e| {
            tracing::error!("Session serialization error: {}", e);
            ApiError::internal_server_error("Session serialization failed")
        })?;

        conn.set_ex::<_, _, ()>(key, value, ttl_secs as u64)
            .await
            .map_err(|e| {
                tracing::error!("Failed to store session in cache: {}", e);
                ApiError::internal_server_error("Failed to store session")
            })?;

        Ok(())
    }

    /// Get session from Dragonfly (fast path)
    async fn get_session_from_cache(&self, token_hash: &str) -> Result<SessionData, ApiError> {
        let mut conn = self.cache.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let key = format!("session:{}", token_hash);
        let value: Option<String> = conn.get(&key).await.map_err(|e| {
            tracing::error!("Failed to get session from cache: {}", e);
            ApiError::internal_server_error("Failed to retrieve session")
        })?;

        let session_json =
            value.ok_or_else(|| ApiError::unauthorized("Session not found or expired"))?;

        let session: SessionData = serde_json::from_str(&session_json).map_err(|e| {
            tracing::error!("Session deserialization error: {}", e);
            ApiError::internal_server_error("Session data corrupted")
        })?;

        Ok(session)
    }

    /// Verify and get user from access token (Dragonfly first)
    pub async fn verify_access_token(&self, token: &str) -> Result<SessionData, ApiError> {
        let token_hash = Self::hash_token(token);

        // Fast path: Check Dragonfly
        match self.get_session_from_cache(&token_hash).await {
            Ok(session) => {
                tracing::debug!("Session cache hit");
                return Ok(session);
            }
            Err(e) if e.status == 401 => {
                // Session not in cache, check Postgres (fallback)
                tracing::debug!("Session cache miss, checking Postgres");
            }
            Err(e) => return Err(e),
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

        let remaining_ttl = (session_db.expires_at - chrono::Utc::now())
            .num_seconds()
            .max(0);
        if remaining_ttl > 0
            && let Err(e) = self
                .store_session_in_cache(&token_hash, &session_data, remaining_ttl)
                .await
        {
            tracing::warn!("Failed to repopulate cache: {}", e);
        }

        Ok(session_data)
    }

    /// Refresh access token using refresh token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenPair, ApiError> {
        let refresh_token_hash = Self::hash_token(refresh_token);

        // Find refresh token (only in Postgres)
        let token =
            vaultless_core::models::auth::RefreshToken::find_by_hash(&self.db, &refresh_token_hash)
                .await
                .map_err(|_| ApiError::unauthorized("Invalid refresh token"))?;

        // Check if already used (theft detection)
        if token.is_used {
            tracing::warn!(
                user_id = %token.user_id,
                token_family = %token.token_family,
                "Refresh token reuse detected - revoking family"
            );

            vaultless_core::models::auth::RefreshToken::revoke_family(&self.db, token.token_family)
                .await
                .map_err(ApiError::from)?;

            return Err(ApiError::forbidden(
                "Token reuse detected - all tokens revoked",
            ));
        }

        if token.is_revoked {
            return Err(ApiError::unauthorized("Refresh token has been revoked"));
        }

        if token.expires_at < chrono::Utc::now() {
            return Err(ApiError::unauthorized("Refresh token expired"));
        }

        // Get user info
        let user = vaultless_core::models::auth::User::find_by_id(&self.db, token.user_id)
            .await
            .map_err(ApiError::from)?;

        let new_access_token = Self::generate_token().map_err(|e| {
            tracing::error!("Failed to generate new access token: {}", e);
            ApiError::internal_server_error(format!("Failed to generate access token: {}", e))
        })?;

        let new_refresh_token = Self::generate_token().map_err(|e| {
            tracing::error!("Failed to generate access token: {}", e);
            ApiError::internal_server_error(format!("Failed to generate refresh token: {}", e))
        })?;

        let new_access_hash = Self::hash_token(&new_access_token);
        let new_refresh_hash = Self::hash_token(&new_refresh_token);

        // Rotate refresh token in Postgres
        vaultless_core::models::auth::RefreshToken::rotate(&self.db, token.id, new_refresh_hash)
            .await
            .map_err(ApiError::from)?;

        let access_ttl = 3600;
        let now = chrono::Utc::now().timestamp();

        // Store new session in Dragonfly
        let session_data = SessionData {
            user_id: user.id.to_string(),
            email: user.email.clone(),
            scope: None,
            is_admin: user.is_admin,
            created_at: now,
        };

        self.store_session_in_cache(&new_access_hash, &session_data, access_ttl)
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

        // Remove from cache immediately
        let mut conn = self.cache.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        let key = format!("session:{}", token_hash);
        let _: () = conn.del(&key).await.map_err(|e| {
            tracing::error!("Failed to delete session from cache: {}", e);
            ApiError::internal_server_error("Failed to revoke session")
        })?;

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
        // Remove all sessions from cache
        let mut conn = self.cache.get().await.map_err(|e| {
            tracing::error!("Cache connection error: {}", e);
            ApiError::internal_server_error("Cache unavailable")
        })?;

        // Find all session keys for this user
        let pattern = "session:*".to_string();
        let keys: Vec<String> = conn.keys(&pattern).await.map_err(|e| {
            tracing::error!("Failed to get session keys: {}", e);
            ApiError::internal_server_error("Failed to list sessions")
        })?;

        // Delete matching sessions (need to check user_id)
        for key in keys {
            if let Ok(Some(data)) = conn.get::<_, Option<String>>(&key).await
                && let Ok(session) = serde_json::from_str::<SessionData>(&data)
                && session.user_id == user_id.to_string()
            {
                let _: () = conn.del(&key).await.unwrap_or(());
            }
        }

        // Also revoke in Postgres
        vaultless_core::models::auth::UserSession::revoke_all_for_user(&self.db, user_id)
            .await
            .map_err(ApiError::from)?;

        Ok(())
    }
}
