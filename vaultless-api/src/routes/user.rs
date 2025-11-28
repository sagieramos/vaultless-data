use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{AppState, middleware::user::user_auth, handlers::user::*, state};

pub fn user_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Protected routes (requires authenticated user)
        .route("/me", get(get_current_user))
        .route("/logout", post(logout))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            user_auth,
        ))
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
}
