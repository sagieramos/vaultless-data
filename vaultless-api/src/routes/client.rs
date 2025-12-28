use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{
    AppState,
    handlers::clients::{auth::*, handshake},
    middleware::client::client_auth,
};

pub fn client_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Protected endpoints (require auth)
        .route("/me", get(get_current_client).delete(deactivate_client))
        .route("/logout", post(logout))
        // Handshake endpoints for session establishment
        .route("/handshake/initiate", post(handshake::initiate_handshake))
        .route("/handshake/respond", post(handshake::respond_to_handshake))
        .route("/handshake/complete", post(handshake::complete_handshake))
        .layer(middleware::from_fn_with_state(state.clone(), client_auth))
        // Public routes
        .route("/register", post(sign_up_client))
        .route("/authenticate", post(login_client))
        .route("/challenge", get(generate_challenge))
        .route("/lookup", get(lookup_client))
        .route("/health", get(health_check))
}
