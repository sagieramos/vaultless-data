use serde::{Deserialize, Serialize};
use validator::Validate;

// ============================================================================
// REGISTRATION
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,

    #[validate(length(
        min = 2,
        max = 255,
        message = "Name must be between 2 and 255 characters"
    ))]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub email: String,
    pub message: String,
}

// ============================================================================
// LOGIN
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,

    #[validate(length(min = 1, message = "Password is required"))]
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub user: UserInfo,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub email: String,
    pub name: Option<String>,
    pub email_verified: bool,
    pub is_admin: bool,
}

// ============================================================================
// TOKEN REFRESH
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct RefreshTokenRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct RefreshTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

// ============================================================================
// EMAIL VERIFICATION
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyEmailResponse {
    pub message: String,
    pub email: String,
}

// ============================================================================
// PASSWORD RESET
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct RequestPasswordResetRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResendVerificationRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct RequestPasswordResetResponse {
    pub message: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResetPasswordRequest {
    pub token: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct ResetPasswordResponse {
    pub message: String,
}

// ============================================================================
// LOGOUT
// ============================================================================

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub message: String,
}

// ============================================================================
// CURRENT USER
// ============================================================================

#[derive(Debug, Serialize)]
pub struct CurrentUserResponse {
    pub user: UserInfo,
}
