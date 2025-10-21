use crate::{
    handlers::proofs::*,
    middleware::{
        api_key_auth::require_client_api_key,
        rate_limit::{rate_limit_by_api_key, rate_limit_by_ip},
    },
    state::AppState,
};
use axum::{
    Router,
    routing::{get, post},
};

pub fn proof_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // --------------------------------------------------------------------
        // API clients (must provide API key)
        // --------------------------------------------------------------------
        .route("/messages/:message_id/proof", post(create_proof))
        .route("/messages/:message_id/verify", post(verify_message_proof))
        .route("/messages/:message_id/proof", get(get_message_proof))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_client_api_key,
        ))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_by_api_key,
        ))
        // --------------------------------------------------------------------
        // Public endpoint (anyone can verify by content hash)
        // --------------------------------------------------------------------
        .route("/proofs/by-hash/:content_hash", get(find_proofs_by_hash))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            rate_limit_by_ip,
        ))
}
