use crate::AppState;
use crate::{handlers::api_keys::*, middleware::user::require_user_auth};
use axum::{
    Router, middleware,
    routing::{delete, get, patch, post},
};

// API Keys routes with router-level auth middleware
pub fn api_key_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/", post(create_api_key).get(list_api_keys))
        .route("/{key_id}", get(get_api_key).patch(update_api_key))
        .route("/{key_id}/revoke", delete(revoke_api_key))
        .route("/{key_id}/deactivate", post(deactivate_api_key))
        .route("/{key_id}/reactivate", post(reactivate_api_key))
        .route("/{key_id}/upgrade", post(upgrade_api_key))
        // Apply user authentication middleware to all routes
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_user_auth,
        ))
}
