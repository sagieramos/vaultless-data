// user handlers: registration, login, logout, token refresh, email verification, password reset
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::StatusCode,
};
use std::net::SocketAddr;
use validator::Validate;
use vaultless_core::models::user::{LoginAttempt, User};

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
        // secure: do not log tokens in production logs
        email_verification_token = %user.email_verification_token.clone().unwrap_or_default(),
        "New user registered"
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

    let ip_address = addr.ip();
    let ip_str = ip_address.to_string();

    // Check rate limiting in Dragonfly (fast!)
    let token_service = TokenService::new(state.db.clone(), state.cache.clone());

    if token_service
        .is_rate_limited(&ip_str)
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
            // Track failure
            let _ = token_service.track_login_failure(&ip_str).await;

            // Log failed attempt in Postgres (async)
            let db = state.db.clone();
            let email = req.email.clone();
            tokio::spawn(async move {
                let _ = LoginAttempt::log(
                    &db,
                    email,
                    ip_address,
                    false,
                    Some("User not found".to_string()),
                )
                .await;
            });

            return Err(ApiError::unauthorized("Invalid email or password"));
        }
    };

    // Verify password
    let password_valid = user
        .verify_password(&req.password)
        .map_err(ApiError::from)?;

    if !password_valid {
        // Track failure
        let _ = token_service.track_login_failure(&ip_str).await;

        // Log failed attempt (async)
        let db = state.db.clone();
        let email = req.email.clone();
        tokio::spawn(async move {
            let _ = LoginAttempt::log(
                &db,
                email,
                ip_address,
                false,
                Some("Invalid password".to_string()),
            )
            .await;
        });

        return Err(ApiError::unauthorized("Invalid email or password"));
    }

    // Check if user is active
    if !user.is_active {
        return Err(ApiError::forbidden("Account is deactivated"));
    }

    // Clear login failures on success
    let _ = token_service.clear_login_failures(&ip_str).await;

    // Log successful attempt (async)
    let db = state.db.clone();
    let email = req.email.clone();
    tokio::spawn(async move {
        let _ = LoginAttempt::log(&db, email, ip_address, true, None).await;
    });

    // Update last login
    let _ = User::update_last_login(&state.db, user.id).await;

    // Create token pair (stored in Dragonfly!)
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
            email: user.email,
            name: user.name,
            email_verified: user.email_verified,
            is_admin: user.is_admin,
        },
    }))
}
