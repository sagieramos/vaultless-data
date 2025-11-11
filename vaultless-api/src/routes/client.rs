use axum::{
    Router, middleware,
    routing::{get, post},
};

use crate::{
    AppState, handlers::client::*, middleware::client::require_authenticated_client, state,
};

pub fn client_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Protected endpoints (require auth)
        .route("/me", get(get_current_client).delete(deactivate_client))
        .route("/logout", post(logout_client))
        // Public endpoints (no auth)
        .route("/register", post(register_client))
        .route("/authenticate", post(authenticate_client))
        .route("/challenge", get(generate_challenge))
        .route("/lookup", get(lookup_client))
        .route("/health", get(health_check))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_authenticated_client,
        ))
}
