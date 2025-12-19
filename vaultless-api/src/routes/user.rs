use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{
    AppState,
    handlers::developer::user_auth::*,
    handlers::developer::google_oauth::*,
    middleware::user::user_auth,
};

pub fn user_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Protected routes (requires authenticated user)
        .route("/me", get(get_current_user))
        .route("/logout", post(logout))
        .layer(middleware::from_fn_with_state(state.clone(), user_auth))
        // Public routes
        .route("/register", post(register))
        .route("/login", post(login))
        .route(
            "/resend-verification-email",
            post(resend_verification_email),
        )
        .route(
            "/verify-email",
            get(verify_email_get).post(verify_email_post),
        )
        .route("/request-password-reset", post(request_password_reset))
        .route("/reset-password", post(reset_password))
        .route("/refresh-token", post(refresh_token))
        // =========================================================================
        // Google OAuth 2.0 Routes
        // =========================================================================
        // GET /auth/google - Redirects to Google consent screen
        .route("/google", get(google_auth_init))
        // GET /auth/google/url - Returns auth URL as JSON (for SPAs)
        .route("/google/url", get(google_auth_url))
        // GET /auth/google/callback - Handles OAuth callback from Google
        .route("/google/callback", get(google_auth_callback))
        // POST /dev/auth/test-token - Generate test token (development only)
        .route("/test-token", post(generate_test_token))
}
