use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::StatusCode,
};
use std::net::SocketAddr;
use validator::Validate;
use vaultless_core::models::auth::{LoginAttempt, User};

use crate::{
    middleware::error::ApiError,
    services::token::{SessionData, TokenService},
    state::AppState,
};

use super::dto::*;

// ============================================================================
// REGISTRATION
// ============================================================================

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
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
        "New user registered"
    );

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            user_id: user.id.to_string(),
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

    let ip_address = addr.ip();

    // Check rate limiting
    if LoginAttempt::is_rate_limited(&state.db, ip_address)
        .await
        .unwrap_or(false)
    {
        return Err(ApiError::too_many_requests(
            "Too many failed login attempts. Please try again later.",
        ));
    }

    // Find user
    let user = User::find_by_email(&state.db, &req.email).await;

    let user = match user {
        Ok(u) => u,
        Err(_) => {
            // Log failed attempt
            let _ = LoginAttempt::log(
                &state.db,
                req.email.clone(),
                ip_address,
                false,
                Some("User not found".to_string()),
            )
            .await;

            return Err(ApiError::unauthorized("Invalid email or password"));
        }
    };

    // Verify password
    let password_valid = user
        .verify_password(&req.password)
        .map_err(ApiError::from)?;

    if !password_valid {
        // Log failed attempt
        let _ = LoginAttempt::log(
            &state.db,
            req.email.clone(),
            ip_address,
            false,
            Some("Invalid password".to_string()),
        )
        .await;

        return Err(ApiError::unauthorized("Invalid email or password"));
    }

    // Check if user is active
    if !user.is_active {
        return Err(ApiError::forbidden("Account is deactivated"));
    }

    // Log successful attempt
    let _ = LoginAttempt::log(&state.db, req.email.clone(), ip_address, true, None).await;

    // Update last login
    let _ = User::update_last_login(&state.db, user.id).await;

    // Create token pair
    let token_service = TokenService::new(state.db.clone(), state.cache.clone());
    let token_pair = token_service
        .create_token_pair(user.id, user.email.clone(), None, user.is_admin)
        .await?;

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
            id: user.id.to_string(),
            email: user.email,
            name: user.name,
            email_verified: user.email_verified,
            is_admin: user.is_admin,
        },
    }))
}

// ============================================================================
// REFRESH TOKEN
// ============================================================================

pub async fn refresh_token(
    State(state): State<AppState>,
    Json(req): Json<RefreshTokenRequest>,
) -> Result<Json<RefreshTokenResponse>, ApiError> {
    let token_service = TokenService::new(state.db.clone(), state.cache.clone());

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
    session: SessionData,
) -> Result<Json<LogoutResponse>, ApiError> {
    let token_service = TokenService::new(state.db.clone(), state.cache.clone());

    // Revoke all tokens for this user
    let user_id = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    token_service.revoke_all_user_tokens(user_id).await?;

    tracing::info!(user_id = %session.user_id, "User logged out");

    Ok(Json(LogoutResponse {
        message: "Logged out successfully".to_string(),
    }))
}

// ============================================================================
// EMAIL VERIFICATION
// ============================================================================

pub async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>,
) -> Result<Json<VerifyEmailResponse>, ApiError> {
    let user = User::verify_email(&state.db, &req.token)
        .await
        .map_err(ApiError::from)?;

    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        "Email verified successfully"
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
) -> Result<Json<RequestPasswordResetResponse>, ApiError> {
    req.validate()
        .map_err(|e| ApiError::bad_request(format!("Validation error: {}", e)))?;

    // Generate reset token (don't reveal if email exists)
    let _ = User::request_password_reset(&state.db, &req.email).await;

    // TODO: Send password reset email

    tracing::info!(email = %req.email, "Password reset requested");

    Ok(Json(RequestPasswordResetResponse {
        message: "If the email exists, a password reset link has been sent.".to_string(),
    }))
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
    let token_service = TokenService::new(state.db.clone(), state.cache.clone());
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
    State(state): State<AppState>,
    session: SessionData,
) -> Result<Json<CurrentUserResponse>, ApiError> {
    let user_id = session
        .user_id
        .parse()
        .map_err(|_| ApiError::internal_server_error("Invalid user ID in session"))?;

    let user = User::find_by_id(&state.db, user_id)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CurrentUserResponse {
        user: UserInfo {
            id: user.id.to_string(),
            email: user.email,
            name: user.name,
            email_verified: user.email_verified,
            is_admin: user.is_admin,
        },
    }))
}
