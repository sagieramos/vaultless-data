use axum::{
    routing::{get, post},
    Router,
};

use crate::{handlers::client::*, AppState};

pub fn client_routes() -> Router<AppState> {
    Router::new()
        // Public endpoints (no auth)
        .route("/register", post(register_client))
        .route("/authenticate", post(authenticate_client))
        .route("/challenge", get(generate_challenge))
        .route("/lookup", get(lookup_client))
        .route("/health", get(health_check))
        // Protected endpoints (require auth)
        .route("/me", get(get_current_client).delete(deactivate_client))
        .route("/logout", post(logout_client))
}