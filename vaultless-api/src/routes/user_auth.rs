use axum::{
    Router,
    routing::{get, post},
};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

use crate::{handlers::user_auth::*, middleware::user_auth::require_user_auth, state::AppState};

/// Build `/auth` routes
pub fn auth_routes(state: AppState) -> Router<AppState> {
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
    let send_verification_email_layer = GovernorConfigBuilder::default()
        .per_second(60)
        .burst_size(1)
        .finish()
        .unwrap();

    // Auth-protected routes (require_user_auth)
    let protected = Router::new()
        .route("/logout", post(logout))
        .route("/me", get(get_current_user))
        .route("/logout", get(logout))
        // refresh can optionally be protected (recommended)
        .route("/refresh", post(refresh_token))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ));

    // Public routes
    let public = Router::new()
        .route("/send_verification_email", post(resend_verification_email))
        .layer(GovernorLayer::new(send_verification_email_layer))
        .route("/register", post(register))
        .route("/login", post(login))
        .layer(GovernorLayer::new(strict_limit_layer)) // strict rate limit for login/register
        .route("/verify-email", get(verify_email_get))
        .route("/password/request-reset", post(request_password_reset))
        .route("/password/reset", post(reset_password));

    // Combine routers
    Router::new()
        .merge(public)
        .merge(protected)
        .layer(GovernorLayer::new(rate_limit_layer))
        .with_state(state)
}
