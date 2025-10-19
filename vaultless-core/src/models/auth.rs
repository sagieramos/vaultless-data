use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, postgres::PgPool};
use std::net::IpAddr;
use uuid::Uuid;

use crate::VaultlessError;

struct UserRegistration {
    email: String,
    password: String,
    name: Option<String>,
}

// ============================================================================
// USER MODEL
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub email_verified: bool,
    pub email_verification_token: Option<String>,
    pub email_verification_expires_at: Option<DateTime<Utc>>,
    pub password_reset_token: Option<String>,
    pub password_reset_expires_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub stripe_customer_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl User {
    /// Create a new user with hashed password
    pub async fn create(
        pool: &PgPool,
        email: String,
        password: String,
        name: Option<String>,
    ) -> Result<Self, VaultlessError> {
        let user = UserRegistration {
            email,
            password,
            name,
        };
        // Hash password (bcrypt with cost 12)
        let password_hash = bcrypt::hash(user.password, 12)
            .map_err(|e| VaultlessError::Internal(format!("Password hashing failed: {}", e)))?;

        // Generate email verification token
        let verification_token = Self::generate_token().map_err(|e| {
            VaultlessError::Internal(format!("Failed to generate verification token: {}", e))
        })?;
        let verification_expires = Utc::now() + Duration::hours(24);

        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (email, password_hash, name, email_verification_token, email_verification_expires_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(&user.email)
        .bind(&password_hash)
        .bind(user.name)
        .bind(&verification_token)
        .bind(verification_expires)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                VaultlessError::Conflict("Email already registered".to_string())
            }
            _ => VaultlessError::from(e),
        })?;

        Ok(user)
    }

    /// Find user by ID
    pub async fn find_by_id(pool: &PgPool, user_id: Uuid) -> Result<Self, VaultlessError> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .map_err(|_| VaultlessError::NotFound("User not found".to_string()))
    }

    /// Find user by email
    pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Self, VaultlessError> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(pool)
            .await
            .map_err(|_| VaultlessError::NotFound("User not found".to_string()))
    }

    /// Verify password
    pub fn verify_password(&self, password: &str) -> Result<bool, VaultlessError> {
        bcrypt::verify(password, &self.password_hash)
            .map_err(|e| VaultlessError::Internal(format!("Password verification failed: {}", e)))
    }

    /// Update last login timestamp
    pub async fn update_last_login(pool: &PgPool, user_id: Uuid) -> Result<(), VaultlessError> {
        sqlx::query("UPDATE users SET last_login_at = NOW() WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Verify email with token
    pub async fn verify_email(pool: &PgPool, token: &str) -> Result<Self, VaultlessError> {
        let user = sqlx::query_as::<_, User>(
            r#"
            UPDATE users 
            SET email_verified = true, 
                email_verification_token = NULL,
                email_verification_expires_at = NULL,
                updated_at = NOW()
            WHERE email_verification_token = $1 
                AND email_verification_expires_at > NOW()
            RETURNING *
            "#,
        )
        .bind(token)
        .fetch_one(pool)
        .await
        .map_err(|_| {
            VaultlessError::Unauthorized("Invalid or expired verification token".to_string())
        })?;

        Ok(user)
    }

    /// Request password reset
    pub async fn request_password_reset(
        pool: &PgPool,
        email: &str,
    ) -> Result<String, VaultlessError> {
        let reset_token = Self::generate_token().map_err(|e| {
            VaultlessError::Internal(format!("Failed to generate reset token: {}", e))
        })?;
        let reset_expires = Utc::now() + Duration::hours(1);

        sqlx::query(
            r#"
            UPDATE users 
            SET password_reset_token = $1,
                password_reset_expires_at = $2,
                updated_at = NOW()
            WHERE email = $3 AND is_active = true
            "#,
        )
        .bind(&reset_token)
        .bind(reset_expires)
        .bind(email)
        .execute(pool)
        .await?;

        Ok(reset_token)
    }

    /// Reset password with token
    pub async fn reset_password(
        pool: &PgPool,
        token: &str,
        new_password: String,
    ) -> Result<Self, VaultlessError> {
        let password_hash = bcrypt::hash(new_password, 12)
            .map_err(|e| VaultlessError::Internal(format!("Password hashing failed: {}", e)))?;

        let user = sqlx::query_as::<_, User>(
            r#"
            UPDATE users 
            SET password_hash = $1,
                password_reset_token = NULL,
                password_reset_expires_at = NULL,
                updated_at = NOW()
            WHERE password_reset_token = $2 
                AND password_reset_expires_at > NOW()
            RETURNING *
            "#,
        )
        .bind(&password_hash)
        .bind(token)
        .fetch_one(pool)
        .await
        .map_err(|_| VaultlessError::Unauthorized("Invalid or expired reset token".to_string()))?;

        Ok(user)
    }

    /// Generate secure random token
    fn generate_token() -> Result<String, getrandom::Error> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed)?;
        Ok(URL_SAFE_NO_PAD.encode(seed))
    }
}

// ============================================================================
// USER SESSION MODEL
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserSession {
    pub id: Uuid,
    pub user_id: Uuid,
    /*
    #[serde(skip_serializing)]
    #[sqlx(skip)]
    pub access_token: String,
    */
    pub access_token_hash: String,
    pub token_type: String,
    pub scope: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>, // Changed to String
    pub device_id: Option<String>,
    pub is_active: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl UserSession {
    /// Create new session (only for audit - sessions stored in Dragonfly)
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        access_token_hash: String,
        scope: Option<String>,
        ttl_seconds: i64,
    ) -> Result<Self, VaultlessError> {
        let expires_at = Utc::now() + Duration::seconds(ttl_seconds);

        let session = sqlx::query_as::<_, UserSession>(
            r#"
            INSERT INTO user_sessions 
                (user_id, access_token_hash, scope, expires_at)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(&access_token_hash) // Use the hash as the access_token too (already unique)
        .bind(scope)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(session)
    }

    /// Find session by token hash (fallback - prefer Dragonfly)
    pub async fn find_by_token_hash(
        pool: &PgPool,
        token_hash: &str,
    ) -> Result<Self, VaultlessError> {
        sqlx::query_as::<_, UserSession>(
            "SELECT * FROM user_sessions WHERE access_token_hash = $1 AND is_active = true AND expires_at > NOW()"
        )
        .bind(token_hash)
        .fetch_one(pool)
        .await
        .map_err(|_| VaultlessError::Unauthorized("Invalid or expired session".to_string()))
    }

    /// Revoke session
    pub async fn revoke(pool: &PgPool, session_id: Uuid) -> Result<(), VaultlessError> {
        sqlx::query("UPDATE user_sessions SET is_active = false, revoked_at = NOW() WHERE id = $1")
            .bind(session_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Revoke all sessions for user
    pub async fn revoke_all_for_user(pool: &PgPool, user_id: Uuid) -> Result<(), VaultlessError> {
        sqlx::query(
            "UPDATE user_sessions SET is_active = false, revoked_at = NOW() WHERE user_id = $1 AND is_active = true"
        )
        .bind(user_id)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ============================================================================
// REFRESH TOKEN MODEL
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub session_id: Option<Uuid>,
    pub token_hash: String,
    pub token_family: Uuid,
    pub parent_token_id: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub is_used: bool,
    pub used_at: Option<DateTime<Utc>>,
    pub is_revoked: bool,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    pub device_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl RefreshToken {
    /// Create new refresh token
    pub async fn create(
        pool: &PgPool,
        user_id: Uuid,
        token_hash: String,
        token_family: Uuid,
        ttl_days: i64,
    ) -> Result<Self, VaultlessError> {
        let expires_at = Utc::now() + Duration::days(ttl_days);

        let token = sqlx::query_as::<_, RefreshToken>(
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, token_family, expires_at)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(&token_hash)
        .bind(token_family)
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(token)
    }

    /// Find refresh token by hash
    pub async fn find_by_hash(pool: &PgPool, token_hash: &str) -> Result<Self, VaultlessError> {
        sqlx::query_as::<_, RefreshToken>("SELECT * FROM refresh_tokens WHERE token_hash = $1")
            .bind(token_hash)
            .fetch_one(pool)
            .await
            .map_err(|_| VaultlessError::NotFound("Refresh token not found".to_string()))
    }

    /// Rotate refresh token (mark old as used, create new one in the same family)
    pub async fn rotate(
        pool: &PgPool,
        old_token_id: Uuid,
        new_token_hash: String,
    ) -> Result<Self, VaultlessError> {
        // Get the old token to extract user_id, token_family, and expiry
        let old_token =
            sqlx::query_as::<_, RefreshToken>("SELECT * FROM refresh_tokens WHERE id = $1")
                .bind(old_token_id)
                .fetch_one(pool)
                .await?;

        // Mark old token as used
        sqlx::query("UPDATE refresh_tokens SET is_used = true, used_at = NOW() WHERE id = $1")
            .bind(old_token_id)
            .execute(pool)
            .await?;

        // Create new token in the same family with same expiry duration
        let remaining_ttl = (old_token.expires_at - Utc::now()).num_days();
        let expires_at = Utc::now() + Duration::days(remaining_ttl.max(1)); // At least 1 day

        let new_token = sqlx::query_as::<_, RefreshToken>(
            r#"
            INSERT INTO refresh_tokens 
                (user_id, token_hash, token_family, parent_token_id, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(old_token.user_id)
        .bind(&new_token_hash)
        .bind(old_token.token_family) // Same family for theft detection
        .bind(old_token_id) // Link to parent
        .bind(expires_at)
        .fetch_one(pool)
        .await?;

        Ok(new_token)
    }

    /// Revoke entire token family (theft detection)
    pub async fn revoke_family(pool: &PgPool, token_family: Uuid) -> Result<(), VaultlessError> {
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

// ============================================================================
// LOGIN ATTEMPT MODEL (Security)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct LoginAttempt {
    pub id: Uuid,
    pub email: String,
    pub ip_address: String, // Changed to String
    pub success: bool,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl LoginAttempt {
    /// Log login attempt
    pub async fn log(
        pool: &PgPool,
        email: String,
        ip_address: IpAddr,
        success: bool,
        failure_reason: Option<String>,
    ) -> Result<(), VaultlessError> {
        sqlx::query(
            r#"
            INSERT INTO login_attempts (email, ip_address, success, failure_reason)
            VALUES ($1, $2::inet, $3, $4)
            "#,
        )
        .bind(email)
        .bind(ip_address.to_string()) // Convert to String, cast to inet
        .bind(success)
        .bind(failure_reason)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Check if IP is rate limited (5 failures in 15 minutes)
    pub async fn is_rate_limited(
        pool: &PgPool,
        ip_address: IpAddr,
    ) -> Result<bool, VaultlessError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) 
            FROM login_attempts 
            WHERE ip_address = $1::inet
                AND success = false 
                AND created_at > NOW() - INTERVAL '15 minutes'
            "#,
        )
        .bind(ip_address.to_string()) // Convert to String, cast to inet
        .fetch_one(pool)
        .await?;

        Ok(count >= 5)
    }
}
