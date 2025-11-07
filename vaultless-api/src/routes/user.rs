use axum::{
    Router,
    routing::{get, post},
};

use crate::{AppState, handlers::user::*};

pub fn user_routes() -> Router<AppState> {
    Router::new()
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
        // Protected routes (requires authenticated user)
        .route("/me", get(get_current_user))
        .route("/logout", post(logout))
}
