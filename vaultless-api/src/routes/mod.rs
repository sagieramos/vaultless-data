pub mod analytics;
pub mod health;
pub mod notifications;

use axum::{
    Router,
    routing::{delete, get, post},
};

use crate::{
    handlers::{api_keys, auth, messages, proofs},
    middleware::{api_key_auth::require_client_api_key, token_auth::require_user_auth},
    state::AppState,
};

/// Builds the complete API router with nested sub-routes for modularity.
pub fn build_routes(state: AppState) -> Router {
    Router::new()
        // Public health check endpoint.
        .route("/health", get(health::health_check))
        // Nested auth routes (public and protected).
        .nest("/auth", auth_routes(state.clone()))
        // Token-protected dashboard routes.
        .nest("/dashboard", dashboard_routes(state.clone()))
        // Versioned API routes.
        .nest("/api/v1", api_v1_routes(state.clone()))
        .with_state(state)
}

/// Public and token-protected authentication routes.
fn auth_routes(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/me", get(auth::get_current_user))
        .route("/logout", post(auth::logout))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_user_auth,
        ));

    let public = Router::new()
        .route("/register", post(auth::register))
        .route("/login", post(auth::login))
        .route("/refresh", post(auth::refresh_token))
        .route("/verify-email", post(auth::verify_email))
        .route(
            "/request-password-reset",
            post(auth::request_password_reset),
        )
        .route("/reset-password", post(auth::reset_password));

    public.merge(protected)
}

/// Version 1 API routes, grouped by resource with appropriate auth.
fn api_v1_routes(state: AppState) -> Router<AppState> {
    // API-key protected message operations.
    let message_routes = Router::new()
        // More specific routes first to avoid shadowing.
        .route(
            "/{message_id}/metadata",
            get(messages::get_message_metadata),
        )
        .route("/{message_id}/proof", post(proofs::create_proof))
        .route("/{message_id}/proof", get(proofs::get_message_proof))
        .route("/{message_id}/verify", post(proofs::verify_message_proof))
        .route("/{recipient_id}", get(messages::receive_messages)) // Less specific, last.
        .route("/send", post(messages::send_message))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_client_api_key,
        ));

    // Public proof lookup (no auth).
    let public_proof_routes =
        Router::new().route("/by-hash/{content_hash}", get(proofs::find_proofs_by_hash));

    Router::new()
        .nest("/messages", message_routes)
        //.nest("/analytics", analytics_routes)
        .nest("/proofs", public_proof_routes) // Nested for consistency.
}

/// Token-protected dashboard for API key management.
/// Base URL: /dashboard/api-keys
fn dashboard_routes(state: AppState) -> Router<AppState> {
    let key_routes = Router::new()
        .route("/", post(api_keys::create_api_key)) // POST: Create new key.
        .route("/", get(api_keys::list_api_keys)) // GET: List keys.
        .route("/{key_id}", get(api_keys::get_api_key))
        .route("/{key_id}", delete(api_keys::revoke_api_key))
        .route("/{key_id}/deactivate", post(api_keys::deactivate_api_key))
        .route("/{key_id}/reactivate", post(api_keys::reactivate_api_key))
        .route("/{key_id}/upgrade", post(api_keys::upgrade_api_key))
        .layer(axum::middleware::from_fn_with_state(
            state,
            require_user_auth,
        ));

    Router::new().nest("/apikeys", key_routes)
}
