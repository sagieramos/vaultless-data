use axum::{
    Router,
    routing::{get, post},
};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

use crate::{handlers::auth::*, middleware::token_auth::require_user_auth, state::AppState};

/// Build `/auth` routes
pub fn auth_routes(state: AppState) -> Router {
    // Global rate limiter for auth actions
    let rate_limit_layer = GovernorConfigBuilder::default()
        .per_second(3)
        .burst_size(6)
        .finish()
        .unwrap();

    // Stricter rate limit for registration/login to mitigate brute-force
    let strict_limit_layer = GovernorConfigBuilder::default()
        .per_second(1)
        .burst_size(2)
        .finish()
        .unwrap();

    // Auth-protected routes (require_user_auth)
    let protected = Router::new()
        .route("/logout", post(logout))
        .route("/me", get(get_current_user))
        // refresh can optionally be protected (recommended)
        .route("/refresh", post(refresh_token))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ));

    // Public routes
    let public = Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .layer(GovernorLayer::new(strict_limit_layer)) // strict rate limit for login/register
        .route("/verify-email", post(verify_email))
        .route("/password/request-reset", post(request_password_reset))
        .route("/password/reset", post(reset_password));

    // Combine routers
    Router::new()
        .nest("/auth", public)
        .merge(protected)
        .layer(GovernorLayer::new(rate_limit_layer))
        .with_state(state)
}
