// user handlers: registration, login, logout, token refresh, email verification, password reset
use axum::{
    Extension, Json,
    extract::{ConnectInfo, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use std::net::SocketAddr;
use uuid::Uuid;
use validator::Validate;
use vaultless_core::models::user::User;

use crate::{
    middleware::{error::ApiError, user::UserExt},
    services::token::{SessionData, TokenService},
    state::AppState,
};

use axum::extract::Query;
use std::collections::HashMap;
use vaultless_core::VaultlessError;

use super::dto::*;

#[derive(Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub email_verified: bool,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            avatar_url: user.avatar_url,
            email_verified: user.email_verified,
            is_active: user.is_active,
            is_admin: user.is_admin,
            created_at: user.created_at,
            updated_at: user.updated_at,
            last_login_at: user.last_login_at,
            metadata: user.metadata,
        }
    }
}

// ============================================================================
// REGISTRATION
// ============================================================================

pub async fn register(
    State(state): State<AppState>,
    /*  ConnectInfo(addr): ConnectInfo<SocketAddr>, */
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), ApiError> {
    // Validate request
    req.validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    // Create user
    let user = User::create(&state.db, req.email.clone(), req.password, req.name)
        .await
        .map_err(ApiError::from)?;

    // TODO: Send verification email with user.email_verification_token

    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        // secure: do not log url in production logs
        "User registered successfully"
    );

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            email: user.email.clone(),
            message: "Registration successful. Please check your email to verify your account."
                .to_string(),
        }),
    ))
}

// ============================================================================
// LOGIN
// ============================================================================
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    // Validate request
    req.validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    let ip = addr.ip();
    let ip_str = ip.to_string();

    let token_service = TokenService::new(state.db.clone(), state.redis_pool.clone());

    if token_service
        .is_rate_limited(&ip_str)
        .await
        .unwrap_or(false)
    {
        return Err(ApiError::too_many_requests(
            "Too many failed login attempts. Please try again later.",
        ));
    }

    let user_result = User::login_with_transaction(&state.db, &req.email, &req.password, ip).await;

    let user = match user_result {
        Ok(u) => {
            let _ = token_service.clear_login_failures(&ip_str).await;
            u
        }
        Err(VaultlessError::EmailNotVerified(token)) => {
            let verification_url = token
                .as_ref()
                .map(|t| format!("http://localhost:8080/auth/verify-email?token={}", t))
                .unwrap_or_else(|| "Verification link unavailable (token is None)".to_string());

            // TODO. Send email
            tracing::info!(
                verification_url = %verification_url,
                "User email not verified."
            );

            // TODO: Send verification email using email service
            return Err(ApiError::unauthorized(
                "Email not verified. Verification email has been resent.".to_string(),
            ));
        }
        Err(e) => {
            let _ = token_service.track_login_failure(&ip_str).await;
            return Err(ApiError::from(e));
        }
    };
    let token_pair = token_service
        .create_token_pair(user.id, None, user.is_admin)
        .await
        .map_err(|e| {
            tracing::error!(
                user_id = %user.id,
                email = %user.email,
                error = %e,
                "Failed to create token pair after successful login"
            );
            ApiError::internal_server_error("Failed to generate tokens".to_string())
        })?;

    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        "User logged in successfully"
    );

    Ok(Json(LoginResponse {
        access_token: token_pair.access_token,
        refresh_token: token_pair.refresh_token,
        token_type: token_pair.token_type,
        expires_in: token_pair.expires_in,
        user: UserInfo {
            email: user.email,
            name: user.name,
            email_verified: user.email_verified,
            is_admin: user.is_admin,
        },
    }))
}

/// ============================================================================
/// RESEND VERIFICATION EMAIL
/// ============================================================================
/// Example: POST /resend-verification-email { "email": "<youremail@gmail.com>"}
pub async fn resend_verification_email(
    State(state): State<AppState>,
    Json(req): Json<ResendVerificationRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (email, token) = User::resend_verification_token(&state.db, &req.email)
        .await
        .map_err(ApiError::from)?;

    // TODO: send email notification using your email service
    tracing::info!(
        email = %email,
        token = %token
    );

    Ok(Json(serde_json::json!({
        "message": "Verification email resent successfully",
        "email": email,
    })))
}

// ============================================================================
// REFRESH TOKEN
// ============================================================================

pub async fn refresh_token(
    State(state): State<AppState>,
    Json(req): Json<RefreshTokenRequest>,
) -> Result<Json<RefreshTokenResponse>, ApiError> {
    let token_service = TokenService::new(state.db.clone(), state.redis_pool.clone());

    let token_pair = token_service.refresh_token(&req.refresh_token).await?;

    Ok(Json(RefreshTokenResponse {
        access_token: token_pair.access_token,
        refresh_token: token_pair.refresh_token,
        token_type: token_pair.token_type,
        expires_in: token_pair.expires_in,
    }))
}

// ============================================================================
// LOGOUT
// ============================================================================

pub async fn logout(
    State(state): State<AppState>,
    Extension(session): Extension<SessionData>,
) -> Result<Json<LogoutResponse>, ApiError> {
    let token_service = TokenService::new(state.db.clone(), state.redis_pool.clone());

    // Revoke all tokens for this user
    let user_id = session.user_id;
    token_service.revoke_all_user_tokens(user_id).await?;

    tracing::info!(user_id = %session.user_id, "User logged out");

    Ok(Json(LogoutResponse {
        message: "Logged out successfully".to_string(),
    }))
}

// ============================================================================
// EMAIL VERIFICATION
// ============================================================================

/// ============================================================================
/// GET HANDLER (For email link click)
/// ============================================================================
/// Example: GET /verify-email?token=abc123
pub async fn verify_email_get(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let token = params
        .get("token")
        .ok_or_else(|| ApiError::bad_request("Missing verification token"))?;

    let user = User::verify_email(&state.db, token)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        "Email verified successfully via GET"
    );

    Ok(Json(json!({
        "status": "success",
        "message": format!("Email verified successfully for {}", user.email),
    })))
}
/// ============================================================================
/// POST HANDLER (For API / Mobile clients)
/// ============================================================================
/// Example: POST /api/verify-email { "token": "abc123" }
pub async fn verify_email_post(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> Result<Json<VerifyEmailResponse>, ApiError> {
    let user = User::verify_email(&state.db, &req.token)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        "Email verified successfully via POST"
    );

    Ok(Json(VerifyEmailResponse {
        message: "Email verified successfully".to_string(),
        email: user.email,
    }))
}

// ============================================================================
// PASSWORD RESET REQUEST
// ============================================================================

pub async fn request_password_reset(
    State(state): State<AppState>,
    Json(req): Json<RequestPasswordResetRequest>,
) -> Result<(StatusCode, Json<RequestPasswordResetResponse>), ApiError> {
    req.validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    match User::request_password_reset(&state.db, &req.email).await {
        Ok(Some(reset_token)) => {
            tracing::info!(
                email = %req.email,
                password_reset_token = %reset_token,
                "Password reset token generated"
            );

            // TODO: Send password reset email

            Ok((
                StatusCode::OK,
                Json(RequestPasswordResetResponse {
                    message: "Password reset token generated successfully.".to_string(),
                }),
            ))
        }
        Ok(None) => {
            tracing::warn!(email = %req.email, "No active account found for password reset");
            Err(ApiError::not_found(
                "No active account found for the provided email",
            ))
        }
        Err(e) => {
            tracing::error!(email = %req.email, error = %e, "Password reset failed");
            Err(ApiError::internal_server_error(
                "Failed to process password reset request",
            ))
        }
    }
}

// ============================================================================
// PASSWORD RESET
// ============================================================================

pub async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<ResetPasswordResponse>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    let user = User::reset_password(&state.db, &req.token, req.new_password)
        .await
        .map_err(ApiError::from)?;

    // Revoke all existing sessions (force re-login)
    let token_service = TokenService::new(state.db.clone(), state.redis_pool.clone());
    let _ = token_service.revoke_all_user_tokens(user.id).await;

    tracing::info!(user_id = %user.id, "Password reset successfully");

    Ok(Json(ResetPasswordResponse {
        message: "Password reset successfully. Please log in with your new password.".to_string(),
    }))
}

// ============================================================================
// GET CURRENT USER
// ============================================================================

pub async fn get_current_user(
    State(_state): State<AppState>,
    UserExt(user): UserExt,
) -> Result<Json<UserResponse>, ApiError> {
    Ok(Json(UserResponse::from(user)))
}
