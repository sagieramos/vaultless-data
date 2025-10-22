use crate::{
    handlers::user_auth::*,
    middleware::{
        rate_limit::{rate_limit_by_api_key, rate_limit_by_ip},
        token_auth::require_user_auth,
    },
    state::AppState,
};
use axum::{
    Router, middleware,
    routing::{get, post},
};

pub fn auth_routes(state: AppState) -> Router<AppState> {
    // -----------------------------
    // Public endpoints (no auth required)
    // -----------------------------
    let public_routes = Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh_token))
        .route("/verify-email", post(verify_email))
        .route("/password/request-reset", post(request_password_reset))
        .route("/password/reset", post(reset_password))
        // Apply IP-based rate limiting for all public endpoints
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_ip,
        ));

    // -----------------------------
    // Protected endpoints (require user auth)
    // -----------------------------
    let protected_routes = Router::new()
        .route("/logout", post(logout))
        .route("/me", get(get_current_user))
        // Apply user auth first
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ))
        // Then apply API key rate limiting if applicable
        .layer(middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_api_key,
        ));

    // -----------------------------
    // Combine public + protected routes
    // -----------------------------
    Router::new()
        .nest("/", public_routes)
        .nest("/", protected_routes)
        .with_state(state)
}
