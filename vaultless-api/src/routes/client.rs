use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{AppState, handlers::clients::auth::*, middleware::client::client_auth};

pub fn client_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Protected endpoints (require auth)
        .route("/me", get(get_current_client).delete(deactivate_client))
        .route("/logout", post(logout))
        .layer(middleware::from_fn_with_state(state.clone(), client_auth))
        // Public routes
        .route("/register", post(register_client))
        .route("/authenticate", post(login))
        .route("/challenge", get(generate_challenge))
        .route("/lookup", get(lookup_client))
        .route("/health", get(health_check))
}
