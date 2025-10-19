use axum::{Router, routing::post, routing::get};
use crate::handlers::auth::*;
use crate::state::AppState;

pub fn auth_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh_token))
        .route("/logout", post(logout))
        .route("/verify-email", post(verify_email))
        .route("/password/request-reset", post(request_password_reset))
        .route("/password/reset", post(reset_password))
        .route("/me", get(get_current_user))
        .with_state(state)
}
