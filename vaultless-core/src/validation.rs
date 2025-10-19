use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, postgres::PgPool};
use std::net::IpAddr;
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::VaultlessError;

// ============================================================================
// VALIDATION FUNCTIONS
// ============================================================================

/// Validate email format and common restrictions
fn validate_email(email: &str) -> Result<(), ValidationError> {
    // Basic email format validation
    if !validator::validate_email(email) {
        let mut error = ValidationError::new("invalid_email");
        error.message = Some("Please provide a valid email address".into());
        return Err(error);
    }

    // Check email length
    if email.len() > 254 {
        let mut error = ValidationError::new("email_too_long");
        error.message = Some("Email address cannot exceed 254 characters".into());
        return Err(error);
    }

    // Check for disposable/temporary emails
    let disposable_domains = [
        "tempmail",
        "throwaway",
        "fake",
        "guerrillamail",
        "mailinator",
        "10minutemail",
        "temp-mail",
        "yopmail",
        "trashmail",
    ];

    let email_lower = email.to_lowercase();
    for domain in disposable_domains.iter() {
        if email_lower.contains(domain) {
            let mut error = ValidationError::new("disposable_email");
            error.message = Some("Disposable email addresses are not allowed".into());
            return Err(error);
        }
    }

    Ok(())
}

/// Validate strong password requirements
fn validate_password(password: &str) -> Result<(), ValidationError> {
    let mut errors = Vec::new();

    // Check minimum length
    if password.len() < 8 {
        errors.push("at least 8 characters");
    }

    // Check maximum length
    if password.len() > 128 {
        errors.push("no more than 128 characters");
    }

    // Check for uppercase letters
    if !password.chars().any(|c| c.is_ascii_uppercase()) {
        errors.push("at least one uppercase letter (A-Z)");
    }

    // Check for lowercase letters
    if !password.chars().any(|c| c.is_ascii_lowercase()) {
        errors.push("at least one lowercase letter (a-z)");
    }

    // Check for numbers
    if !password.chars().any(|c| c.is_ascii_digit()) {
        errors.push("at least one number (0-9)");
    }

    // Compile once at startup
    static SPECIAL_CHAR_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"[!@#$%^&*()_+\-=\[\]{};':"\\|,.<>?]"#)
            .expect("Failed to compile SPECIAL_CHAR_RE regex")
    });
    if !SPECIAL_CHAR_RE.is_match(password) {
        errors.push("at least one special character (!@#$%^&* etc.)");
    }

    // Check for common weak patterns
    let weak_patterns = [
        "123", "abc", "password", "qwerty", "admin", "welcome", "letmein",
    ];

    let password_lower = password.to_lowercase();
    for pattern in weak_patterns.iter() {
        if password_lower.contains(pattern) {
            errors.push(&format!("cannot contain common pattern '{}'", pattern));
            break;
        }
    }

    // Check for sequential characters
    if has_sequential_chars(password, 3) {
        errors.push("cannot have 3 or more sequential characters");
    }

    // Check for repeated characters
    if has_repeated_chars(password, 4) {
        errors.push("cannot have 4 or more repeated characters");
    }

    if !errors.is_empty() {
        let error_message = format!("Password must contain: {}", errors.join(", "));

        let mut validation_error = ValidationError::new("strong_password");
        validation_error.message = Some(error_message.into());
        return Err(validation_error);
    }

    Ok(())
}

/// Validate user name
fn validate_name(name: &str) -> Result<(), ValidationError> {
    // Allow empty names (optional field)
    if name.is_empty() {
        return Ok(());
    }

    let mut errors = Vec::new();

    // Check minimum length
    if name.len() < 2 {
        errors.push("at least 2 characters");
    }

    // Check maximum length
    if name.len() > 50 {
        errors.push("no more than 50 characters");
    }

    // Check for valid characters (letters, spaces, hyphens, apostrophes)
    let name_re = Regex::new(r"^[a-zA-Z\s\-'\.]+$").unwrap();
    if !name_re.is_match(name) {
        errors.push("only letters, spaces, hyphens, apostrophes, and periods allowed");
    }

    // Check for excessive special characters
    let special_count = name
        .chars()
        .filter(|c| !c.is_alphabetic() && !c.is_whitespace())
        .count();
    if special_count > 3 {
        errors.push("too many special characters");
    }

    // Check for consecutive special characters
    if has_consecutive_special_chars(name) {
        errors.push("cannot have consecutive special characters");
    }

    if !errors.is_empty() {
        let error_message = format!("Name must contain: {}", errors.join(", "));

        let mut validation_error = ValidationError::new("invalid_name");
        validation_error.message = Some(error_message.into());
        return Err(validation_error);
    }

    Ok(())
}

/// Helper function to check for sequential characters
fn has_sequential_chars(s: &str, seq_len: usize) -> bool {
    let chars: Vec<char> = s.chars().collect();

    for window in chars.windows(seq_len) {
        if window.windows(2).all(|pair| {
            let current = pair[0] as u32;
            let next = pair[1] as u32;
            next == current + 1
        }) {
            return true;
        }
    }
    false
}

/// Helper function to check for repeated characters
fn has_repeated_chars(s: &str, repeat_count: usize) -> bool {
    let chars: Vec<char> = s.chars().collect();

    for window in chars.windows(repeat_count) {
        if window.iter().all(|&c| c == window[0]) {
            return true;
        }
    }
    false
}

/// Helper function to check for consecutive special characters
fn has_consecutive_special_chars(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();

    for window in chars.windows(2) {
        if !window[0].is_alphabetic()
            && !window[0].is_whitespace()
            && !window[1].is_alphabetic()
            && !window[1].is_whitespace()
        {
            return true;
        }
    }
    false
}

// ============================================================================
// VALIDATION STRUCTS
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UserRegistration {
    #[validate(custom = "validate_email")]
    pub email: String,

    #[validate(custom = "validate_password")]
    pub password: String,

    #[validate(custom = "validate_name")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct PasswordChange {
    pub current_password: String,

    #[validate(custom = "validate_password")]
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ProfileUpdate {
    #[validate(custom = "validate_email")]
    pub email: String,

    #[validate(custom = "validate_name")]
    pub name: Option<String>,
}

// ============================================================================
// UPDATED USER MODEL WITH VALIDATION
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
    /// Create a new user with validation and hashed password
    pub async fn create(
        pool: &PgPool,
        email: String,
        password: String,
        name: Option<String>,
    ) -> Result<Self, VaultlessError> {
        // Validate input using our validation struct
        let registration = UserRegistration {
            email: email.clone(),
            password: password.clone(),
            name: name.clone(),
        };

        if let Err(validation_errors) = registration.validate() {
            return Err(VaultlessError::Validation(validation_errors));
        }

        // Hash password (bcrypt with cost 12)
        let password_hash = bcrypt::hash(password, 12)
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
        .bind(&email)
        .bind(&password_hash)
        .bind(name)
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

    /// Update user profile with validation
    pub async fn update_profile(
        pool: &PgPool,
        user_id: Uuid,
        email: String,
        name: Option<String>,
    ) -> Result<Self, VaultlessError> {
        // Validate input
        let update = ProfileUpdate {
            email: email.clone(),
            name: name.clone(),
        };

        if let Err(validation_errors) = update.validate() {
            return Err(VaultlessError::Validation(validation_errors));
        }

        let user = sqlx::query_as::<_, User>(
            r#"
            UPDATE users 
            SET email = $1, name = $2, updated_at = NOW()
            WHERE id = $3
            RETURNING *
            "#,
        )
        .bind(&email)
        .bind(name)
        .bind(user_id)
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

    /// Change password with validation
    pub async fn change_password(
        pool: &PgPool,
        user_id: Uuid,
        current_password: String,
        new_password: String,
    ) -> Result<(), VaultlessError> {
        // Validate new password
        let password_change = PasswordChange {
            current_password: current_password.clone(),
            new_password: new_password.clone(),
        };

        if let Err(validation_errors) = password_change.validate() {
            return Err(VaultlessError::Validation(validation_errors));
        }

        // Get user and verify current password
        let user = Self::find_by_id(pool, user_id).await?;

        if !user.verify_password(&current_password)? {
            return Err(VaultlessError::Unauthorized(
                "Current password is incorrect".to_string(),
            ));
        }

        // Hash new password
        let new_password_hash = bcrypt::hash(new_password, 12)
            .map_err(|e| VaultlessError::Internal(format!("Password hashing failed: {}", e)))?;

        // Update password
        sqlx::query(
            r#"
            UPDATE users 
            SET password_hash = $1, updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(&new_password_hash)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// Reset password with validation
    pub async fn reset_password(
        pool: &PgPool,
        token: &str,
        new_password: String,
    ) -> Result<Self, VaultlessError> {
        // Validate new password
        if let Err(validation_errors) = validate_password(&new_password) {
            return Err(VaultlessError::Validation(validation_errors.into()));
        }

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

    // ... rest of your existing User methods (find_by_id, find_by_email, etc.)
    // Keep all your existing methods as they are

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

    // ... all your other existing methods
}

// ============================================================================
// ERROR TYPE EXTENSION
// ============================================================================

// You'll need to update your VaultlessError enum to include Validation:
#[derive(Debug)]
pub enum VaultlessError {
    // ... your existing variants
    Validation(validator::ValidationErrors),
    Conflict(String),
    Unauthorized(String),
    NotFound(String),
    Internal(String),
    // ... etc.
}

impl From<validator::ValidationErrors> for VaultlessError {
    fn from(err: validator::ValidationErrors) -> Self {
        VaultlessError::Validation(err)
    }
}

// Implement Display and Error for VaultlessError as needed
