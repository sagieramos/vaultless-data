use serde::{Deserialize, Serialize};
use validator::Validate;
use utoipa::{IntoParams, ToSchema};

// ============================================================================
// REGISTRATION
// ============================================================================

#[derive(Debug, Deserialize, Validate, ToSchema)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse {
    pub email: String,
    pub message: String,
}

// ============================================================================
// LOGIN
// ============================================================================

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct LoginRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,

    #[validate(length(min = 1, message = "Password is required"))]
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub user: UserInfo,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub email: String,
    pub name: Option<String>,
    pub email_verified: bool,
    pub is_admin: bool,
}

// ============================================================================
// TOKEN REFRESH
// ============================================================================

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

// ============================================================================
// EMAIL VERIFICATION
// ============================================================================

#[derive(Debug, Deserialize, ToSchema)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VerifyEmailResponse {
    pub message: String,
    pub email: String,
}

// ============================================================================
// PASSWORD RESET
// ============================================================================

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RequestPasswordResetRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ResendVerificationRequest {
    #[validate(email(message = "Invalid email address"))]
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestPasswordResetResponse {
    pub message: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    pub token: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub new_password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResetPasswordResponse {
    pub message: String,
}

// ============================================================================
// LOGOUT
// ============================================================================

#[derive(Debug, Serialize, ToSchema)]
pub struct LogoutResponse {
    pub message: String,
}

// ============================================================================
// CURRENT USER
// ============================================================================

#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentUserResponse {
    pub user: UserInfo,
}

// ============================================================================
// GOOGLE OAUTH 2.0
// ============================================================================

/// Query parameters for initiating Google OAuth flow
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAuthQuery {
    /// Optional URL to redirect to after successful authentication
    pub redirect_after: Option<String>,
}

/// Response when initiating Google OAuth flow
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAuthInitResponse {
    /// URL to redirect the user to for Google authentication
    pub auth_url: String,
    /// State token for CSRF protection (included in auth_url)
    pub state: String,
}

/// Query parameters received in Google OAuth callback
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleCallbackQuery {
    /// Authorization code from Google (exchanged for tokens)
    pub code: String,
    /// State token for CSRF validation (must match original)
    pub state: String,
    /// Error code if authentication failed
    pub error: Option<String>,
    /// Error description if authentication failed
    pub error_description: Option<String>,
}

/// Response from successful Google OAuth callback
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAuthResponse {
    /// JWT access token for API authentication
    pub access_token: String,
    /// Refresh token for obtaining new access tokens
    pub refresh_token: String,
    /// Token type (always "Bearer")
    pub token_type: String,
    /// Access token expiry in seconds
    pub expires_in: i64,
    /// Authenticated user information
    pub user: UserInfo,
    /// Whether this is a new user (just registered via Google)
    pub is_new_user: bool,
    /// Optional redirect URL (from initial auth request)
    pub redirect_after: Option<String>,
}

/// Google user profile information (returned for debugging/linking)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleUserProfile {
    /// Google's unique user identifier
    pub google_id: String,
    /// User's email from Google
    pub email: String,
    /// Whether email is verified by Google
    pub email_verified: bool,
    /// User's display name
    pub name: Option<String>,
    /// URL to profile picture
    pub picture: Option<String>,
}

/// Request to link Google account to existing user
#[derive(Debug, Deserialize, ToSchema)]
pub struct LinkGoogleAccountRequest {
    /// Authorization code from Google OAuth callback
    pub code: String,
    /// State token for CSRF validation
    pub state: String,
}

/// Response after linking Google account
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkGoogleAccountResponse {
    pub message: String,
    pub google_profile: GoogleUserProfile,
}

/// Error response for OAuth failures
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAuthError {
    pub error: String,
    pub error_description: Option<String>,
}
