use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Result, VaultlessError};

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub email_verified: bool,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub stripe_customer_id: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct UserSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub access_token_hash: String,
    pub token_type: String,
    pub scope: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub token_family: Uuid,
    pub expires_at: DateTime<Utc>,
    pub is_used: bool,
    pub is_revoked: bool,
    pub created_at: DateTime<Utc>,
}

impl User {
    /// Create new user
    pub async fn create(
        pool: &PgPool,
        email: String,
        password_hash: String,
        name: Option<String>,
    ) -> Result<Self> {
        let user = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO users (email, password_hash, name)
            VALUES ($1, $2, $3)
            RETURNING *
            "#,
        )
        .bind(email)
        .bind(password_hash)
        .bind(name)
        .fetch_one(pool)
        .await?;

        Ok(user)
    }

    /// Find user by email
    pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Self> {
        let user = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM users WHERE email = $1
            "#,
        )
        .bind(email)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("User not found".to_string()))?;

        Ok(user)
    }

    /// Find user by ID
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Self> {
        let user = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM users WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("User not found".to_string()))?;

        Ok(user)
    }

    /// Update last login timestamp
    pub async fn update_last_login(pool: &PgPool, user_id: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE users SET last_login_at = NOW() WHERE id = $1
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Verify email
    pub async fn verify_email(pool: &PgPool, user_id: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE users 
            SET email_verified = true,
                email_verification_token = NULL,
                email_verification_expires_at = NULL
            WHERE id = $1
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }
}

impl UserSession {
    /// Create new session
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        access_token_hash: String,
        scope: Option<String>,
        ttl_seconds: i64,
    ) -> Result<Self> {
        let expires_at = Utc::now() + Duration::seconds(ttl_seconds);

        let session = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO user_sessions (user_id, access_token_hash, scope, expires_at)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(access_token_hash)
        .bind(scope)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(session)
    }

    /// Find session by token hash
    pub async fn find_by_token_hash(pool: &PgPool, token_hash: &str) -> Result<Self> {
        let session = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM user_sessions 
            WHERE access_token_hash = $1 
                AND is_active = true 
                AND expires_at > NOW()
            "#,
        )
        .bind(token_hash)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Session not found or expired".to_string()))?;

        Ok(session)
    }

    /// Revoke session
    pub async fn revoke(pool: &PgPool, session_id: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE user_sessions 
            SET is_active = false, revoked_at = NOW() 
            WHERE id = $1
            "#,
        )
        .bind(session_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Revoke all user sessions
    pub async fn revoke_all_for_user(pool: &PgPool, user_id: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE user_sessions 
            SET is_active = false, revoked_at = NOW() 
            WHERE user_id = $1 AND is_active = true
            "#,
        )
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }
}

impl RefreshToken {
    /// Create new refresh token
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        token_hash: String,
        token_family: Uuid,
        ttl_days: i64,
    ) -> Result<Self> {
        let expires_at = Utc::now() + Duration::days(ttl_days);

        let token = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, token_family, expires_at)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(token_family)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(token)
    }

    /// Find refresh token by hash
    pub async fn find_by_hash(pool: &PgPool, token_hash: &str) -> Result<Self> {
        let token = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM refresh_tokens 
            WHERE token_hash = $1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| VaultlessError::NotFound("Refresh token not found".to_string()))?;

        Ok(token)
    }

    /// Mark token as used and create rotation
    pub async fn rotate(pool: &PgPool, old_token_id: Uuid, new_token_hash: String) -> Result<Self> {
        // Get old token info
        let old_token = sqlx::query_as::<_, Self>(
            r#"
            SELECT * FROM refresh_tokens WHERE id = $1
            "#,
        )
        .bind(old_token_id)
        .fetch_one(pool)
        .await?;

        // Mark old token as used
        sqlx::query(
            r#"
            UPDATE refresh_tokens 
            SET is_used = true, used_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(old_token_id)
        .execute(pool)
        .await?;

        // Create new token in same family
        let new_token = sqlx::query_as::<_, Self>(
            r#"
            INSERT INTO refresh_tokens 
            (user_id, token_hash, token_family, parent_token_id, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(old_token.user_id)
        .bind(new_token_hash)
        .bind(old_token.token_family)
        .bind(old_token_id)
        .bind(old_token.expires_at)
        .fetch_one(pool)
        .await?;

        Ok(new_token)
    }

    /// Revoke token family (detect theft)
    pub async fn revoke_family(pool: &PgPool, token_family: Uuid) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE refresh_tokens 
            SET is_revoked = true, 
                revoked_at = NOW(),
                revoked_reason = 'Token family compromised'
            WHERE token_family = $1 AND is_revoked = false
            "#,
        )
        .bind(token_family)
        .execute(pool)
        .await?;

        Ok(())
    }
}
