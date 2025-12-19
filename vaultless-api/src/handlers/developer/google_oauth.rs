// =============================================================================
// Google OAuth 2.0 Handlers
// =============================================================================
//!
//! HTTP handlers for Google OAuth 2.0 authentication flow.
//!
//! ## Endpoints:
//! - `GET /auth/google` - Initiate OAuth flow (redirects to Google)
//! - `GET /auth/google/callback` - Handle OAuth callback from Google
//!
//! ## Swagger/OpenAPI Testing Notes:
//!
//! **IMPORTANT**: The full Google OAuth flow CANNOT be tested directly in Swagger UI
//! because it requires browser-based redirects to Google's consent screen.
//!
//! ### To test the OAuth flow:
//! 1. Open `GET /auth/google` in a browser (not Swagger)
//! 2. Complete Google sign-in
//! 3. You'll receive a JWT token in the response
//! 4. Use that JWT token in Swagger's "Authorize" button for testing protected endpoints
//!
//! ### For development testing:
//! Use `POST /dev/auth/test-token` to generate a test JWT for Swagger testing
//! (only available in development mode).

use axum::{
    Json,
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use vaultless_core::models::user::User;

use crate::{
    middleware::error::ApiError,
    state::AppState,
};

use super::dto::*;

// =============================================================================
// GET /auth/google - Initiate Google OAuth Flow
// =============================================================================

/// Initiate Google OAuth 2.0 authentication flow
///
/// This endpoint generates a Google OAuth authorization URL and redirects
/// the user to Google's consent screen. After authentication, Google will
/// redirect back to `/auth/google/callback`.
///
/// ## Security:
/// - A CSRF state token is generated and stored server-side
/// - The state token must be validated in the callback
///
/// ## Browser vs API Usage:
/// - **Browser**: Returns HTTP 302 redirect to Google
/// - **API** (Accept: application/json): Returns JSON with auth_url
#[utoipa::path(
    get,
    path = "/auth/google",
    params(GoogleAuthQuery),
    responses(
        (status = 302, description = "Redirect to Google OAuth consent screen"),
        (status = 200, description = "JSON response with auth URL (for API clients)", body = GoogleAuthInitResponse),
        (status = 503, description = "Google OAuth not configured")
    ),
    tag = "Google OAuth"
)]
pub async fn google_auth_init(
    State(state): State<AppState>,
    Query(query): Query<GoogleAuthQuery>,
) -> Result<Response, ApiError> {
    // Check if Google OAuth is configured
    let google_oauth = state.google_oauth.as_ref().ok_or_else(|| {
        tracing::warn!("Google OAuth requested but not configured");
        ApiError::internal_server_error("Google OAuth is not configured")
            .with_code("GOOGLE_OAUTH_NOT_CONFIGURED")
    })?;

    // Generate authorization URL with CSRF state token
    let (auth_url, state_token) = google_oauth
        .generate_auth_url(query.redirect_after)
        .await?;

    tracing::info!(
        state = %state_token,
        "Initiating Google OAuth flow"
    );

    // For browser requests, redirect directly to Google
    // For API requests (could check Accept header), return JSON
    // Default to redirect for OAuth flows
    Ok(Redirect::temporary(&auth_url).into_response())
}

/// Get Google OAuth authorization URL (JSON response)
///
/// Alternative endpoint that always returns JSON instead of redirecting.
/// Useful for SPAs that handle redirects client-side.
#[utoipa::path(
    get,
    path = "/auth/google/url",
    params(GoogleAuthQuery),
    responses(
        (status = 200, description = "Authorization URL generated", body = GoogleAuthInitResponse),
        (status = 503, description = "Google OAuth not configured")
    ),
    tag = "Google OAuth"
)]
pub async fn google_auth_url(
    State(state): State<AppState>,
    Query(query): Query<GoogleAuthQuery>,
) -> Result<Json<GoogleAuthInitResponse>, ApiError> {
    let google_oauth = state.google_oauth.as_ref().ok_or_else(|| {
        ApiError::internal_server_error("Google OAuth is not configured")
            .with_code("GOOGLE_OAUTH_NOT_CONFIGURED")
    })?;

    let (auth_url, state_token) = google_oauth
        .generate_auth_url(query.redirect_after)
        .await?;

    Ok(Json(GoogleAuthInitResponse {
        auth_url,
        state: state_token,
    }))
}

// =============================================================================
// GET /auth/google/callback - Handle Google OAuth Callback
// =============================================================================

/// Handle Google OAuth 2.0 callback
///
/// This endpoint is called by Google after the user completes authentication.
/// It performs the following steps:
///
/// 1. **Validate state token** - Prevents CSRF attacks
/// 2. **Exchange authorization code** - Gets access/refresh tokens from Google
/// 3. **Fetch user profile** - Gets email, name, picture from Google
/// 4. **Find or create user** - Links to existing account or creates new one
/// 5. **Issue JWT tokens** - Returns access/refresh tokens for API auth
///
/// ## Error Handling:
/// - Invalid state → 400 Bad Request (possible CSRF attack)
/// - Google errors → Passed through with error details
/// - User creation failures → 500 Internal Server Error
///
/// ## Response:
/// Returns JWT tokens and user info. The `is_new_user` flag indicates
/// whether a new account was created.
#[utoipa::path(
    get,
    path = "/auth/google/callback",
    params(
        ("code" = String, Query, description = "Authorization code from Google"),
        ("state" = String, Query, description = "CSRF state token"),
        ("error" = Option<String>, Query, description = "Error code if auth failed"),
        ("error_description" = Option<String>, Query, description = "Error description")
    ),
    responses(
        (status = 200, description = "Authentication successful", body = GoogleAuthResponse),
        (status = 400, description = "Invalid callback parameters or CSRF state"),
        (status = 401, description = "Google authentication failed"),
        (status = 500, description = "Internal server error")
    ),
    tag = "Google OAuth"
)]
pub async fn google_auth_callback(
    State(state): State<AppState>,
    Query(query): Query<GoogleCallbackQuery>,
) -> Result<Json<GoogleAuthResponse>, ApiError> {
    // -------------------------------------------------------------------------
    // Step 0: Check for error from Google
    // -------------------------------------------------------------------------
    if let Some(error) = &query.error {
        tracing::warn!(
            error = %error,
            description = ?query.error_description,
            "Google OAuth returned an error"
        );
        return Err(ApiError::bad_request(format!(
            "Google authentication failed: {}",
            query.error_description.as_deref().unwrap_or(error)
        ))
        .with_code("GOOGLE_AUTH_DENIED"));
    }

    // -------------------------------------------------------------------------
    // Step 1: Get Google OAuth service
    // -------------------------------------------------------------------------
    let google_oauth = state.google_oauth.as_ref().ok_or_else(|| {
        ApiError::internal_server_error("Google OAuth is not configured")
    })?;

    // -------------------------------------------------------------------------
    // Step 2: Handle the complete OAuth callback
    // This validates state, exchanges code, and fetches user info
    // -------------------------------------------------------------------------
    let (google_user, _tokens, redirect_after) = google_oauth
        .handle_callback(&query.code, &query.state)
        .await?;

    tracing::info!(
        google_id = %google_user.id,
        email = %google_user.email,
        "Google OAuth callback - user authenticated"
    );

    // -------------------------------------------------------------------------
    // Step 3: Find or create user in our database
    // -------------------------------------------------------------------------
    let (user, is_new_user) = find_or_create_user_from_google(&state, &google_user).await?;

    // -------------------------------------------------------------------------
    // Step 4: Create internal session tokens
    // -------------------------------------------------------------------------
    let token_pair = state
        .token_service
        .create_token_pair(user.id, Some("user".to_string()), user.is_admin)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to create session tokens");
            ApiError::internal_server_error("Failed to create session")
        })?;

    tracing::info!(
        user_id = %user.id,
        email = %user.email,
        is_new_user = is_new_user,
        "Google OAuth login successful"
    );

    // -------------------------------------------------------------------------
    // Step 5: Return response with tokens and user info
    // -------------------------------------------------------------------------
    Ok(Json(GoogleAuthResponse {
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
        is_new_user,
        redirect_after,
    }))
}

// =============================================================================
// Helper: Find or Create User from Google Profile
// =============================================================================

/// Find existing user by email or create a new one from Google profile
///
/// ## Logic:
/// 1. Check if user exists with this email
/// 2. If exists, update Google ID link and return
/// 3. If not exists, create new user with Google profile data
///
/// ## Auto-verification:
/// Users created via Google OAuth have `email_verified = true` because
/// Google has already verified their email ownership.
async fn find_or_create_user_from_google(
    state: &AppState,
    google_user: &crate::services::google_oauth::GoogleUserInfo,
) -> Result<(User, bool), ApiError> {
    // Try to find existing user by email
    let existing_user_result = User::find_by_email(&state.db, &google_user.email).await;

    match existing_user_result {
        Ok(user) => {
            // User exists - update last login and optionally link Google ID
            tracing::debug!(
                user_id = %user.id,
                email = %user.email,
                "Found existing user for Google OAuth"
            );

            // Update last login timestamp
            User::update_last_login(&state.db, user.id)
                .await
                .map_err(ApiError::from)?;

            // TODO: Optionally store google_user.id for future "Login with Google" matching
            // This would require adding a `google_id` column to the users table

            Ok((user, false))
        }
        Err(_) => {
        // Create new user from Google profile
        tracing::info!(
            email = %google_user.email,
            google_id = %google_user.id,
            "Creating new user from Google OAuth"
        );

        // Generate a random password for the user (they'll use Google to login)
        // This prevents password-based login until they set one
        let random_password = uuid::Uuid::new_v4().to_string();

        let user = User::create(
            &state.db,
            google_user.email.clone(),
            random_password,
            google_user.name.clone(),
        )
        .await
        .map_err(ApiError::from)?;

        // Mark email as verified and update avatar (Google verified it)
        // Using raw SQL since vaultless-core doesn't expose these methods
        sqlx::query(
            r#"
            UPDATE users
            SET email_verified = true,
                avatar_url = $2,
                updated_at = NOW()
            WHERE id = $1
            "#
        )
        .bind(user.id)
        .bind(&google_user.picture)
        .execute(state.db.as_ref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to update user from Google OAuth");
            ApiError::internal_server_error("Failed to complete registration")
        })?;

        // Reload user to get updated fields
        let updated_user = User::find_by_id(&state.db, user.id)
            .await
            .map_err(ApiError::from)?;

            Ok((updated_user, true))
        }
    }
}

// =============================================================================
// DEV ONLY: Test Token Generator for Swagger Testing
// =============================================================================

/// Generate a test JWT token for Swagger/API testing (DEVELOPMENT ONLY)
///
/// **WARNING**: This endpoint should ONLY be enabled in development environments.
/// It allows generating valid JWT tokens without authentication, which is a
/// security risk in production.
///
/// ## Usage in Swagger:
/// 1. Call this endpoint to get a test token
/// 2. Click "Authorize" in Swagger UI
/// 3. Enter: `Bearer <token>` in the authorization field
/// 4. Now you can test protected endpoints
#[utoipa::path(
    post,
    path = "/dev/auth/test-token",
    request_body = TestTokenRequest,
    responses(
        (status = 200, description = "Test token generated", body = TestTokenResponse),
        (status = 403, description = "Not available in production")
    ),
    tag = "Development"
)]
pub async fn generate_test_token(
    State(state): State<AppState>,
    Json(req): Json<TestTokenRequest>,
) -> Result<Json<TestTokenResponse>, ApiError> {
    // IMPORTANT: Only allow in development mode
    // You should add an environment check here
    #[cfg(not(debug_assertions))]
    {
        return Err(ApiError::forbidden("Test tokens not available in production")
            .with_code("PRODUCTION_MODE"));
    }

    // Find the user by email
    let user = User::find_by_email(&state.db, &req.email)
        .await
        .map_err(|_| ApiError::not_found("User not found"))?;

    // Create token pair
    let token_pair = state
        .token_service
        .create_token_pair(user.id, Some("user".to_string()), user.is_admin)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to create test token");
            ApiError::internal_server_error("Failed to create token")
        })?;

    Ok(Json(TestTokenResponse {
        access_token: token_pair.access_token,
        refresh_token: token_pair.refresh_token,
        token_type: token_pair.token_type,
        expires_in: token_pair.expires_in,
        message: "Use this token in Swagger's Authorize dialog: Bearer <access_token>".to_string(),
    }))
}

// =============================================================================
// Additional DTOs for Test Token Endpoint
// =============================================================================

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request for generating a test token (development only)
#[derive(Debug, Deserialize, ToSchema)]
pub struct TestTokenRequest {
    /// Email of the user to generate token for
    pub email: String,
}

/// Response containing test token (development only)
#[derive(Debug, Serialize, ToSchema)]
pub struct TestTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub message: String,
}
