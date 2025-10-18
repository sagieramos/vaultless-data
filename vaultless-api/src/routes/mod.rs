pub mod health;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{
    handlers::{analytics, api_keys, auth, messages, proofs},
    middleware::{api_key_auth::require_client_api_key, token_auth::require_token_auth},
    state::AppState,
};

/// Build the complete API router
pub fn build_routes(state: AppState) -> Router {
    Router::new()
        // Health check (no auth required)
        .route("/health", get(health::health_check))
        
        // Auth routes (no auth required for most)
        .nest("/auth", auth_routes(state.clone()))

        // Dashboard routes (require token)
        .nest("/dashboard", dashboard_routes(state.clone()))
        
        // Protected API routes (require API key OR token)
        .nest("/api/v1", api_v1_routes(state.clone()))
        
        .with_state(state)
}

/// Authentication routes (public + protected)
fn auth_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Public auth endpoints
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/refresh", post(auth::refresh_token))
        .route("/verify-email", post(auth::verify_email))
        .route("/request-password-reset", post(auth::request_password_reset))
        .route("/reset-password", post(auth::reset_password))
        
        // Protected auth endpoints (require token)
        .route("/me", get(auth::get_current_user))
        .route("/logout", post(auth::logout))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            require_token_auth,
        ))
}

fn dashboard_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // API key management (for logged-in dashboard users)
        .route("/keys", post(api_keys::create_api_key))
        .route("/keys", get(api_keys::list_api_keys))
        .route("/keys/{key_id}", get(api_keys::get_api_key))
        .route("/keys/{key_id}", delete(api_keys::revoke_api_key))
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            require_token_auth,
        ))
}


/// API v1 routes (protected with API key)
fn api_v1_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Message operations
        .route("/messages/send", post(messages::send_message))
        .route("/messages/{recipient_id}", get(messages::receive_messages))
        .route("/messages/{message_id}/metadata", get(messages::get_message_metadata))
        
        // Proof operations
        .route("/messages/{message_id}/proof", post(proofs::create_proof))
        .route("/messages/{message_id}/proof", get(proofs::get_message_proof))
        .route("/messages/{message_id}/verify", post(proofs::verify_message_proof))
        
        // Public proof lookup (no auth required)
        .route("/proofs/by-hash/{content_hash}", get(proofs::find_proofs_by_hash))
        
        // API key management
        .route("/keys", post(api_keys::create_api_key))
        .route("/keys", get(api_keys::list_api_keys))
        .route("/keys/{key_id}", get(api_keys::get_api_key))
        .route("/keys/{key_id}", delete(api_keys::revoke_api_key))
        
        // Analytics & Usage
/*         .route("/analytics/usage", get(analytics::get_usage_stats))
        .route("/analytics/messages", get(analytics::get_message_stats)) */
        
        // All v1 routes require API key authentication
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_client_api_key,
        ))
}