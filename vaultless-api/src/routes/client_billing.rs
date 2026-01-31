use axum::{
    Router,
    routing::get,
};

use crate::{
    handlers::clients::billing::*,
    middleware::client::client_auth,
    state::AppState,
};

pub fn client_billing_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Client billing overview and history
        .route("/overview", get(get_client_billing_overview))
        .route("/history", get(get_client_billing_history))
        
        // Client usage reports
        .route("/usage", get(get_client_usage_report))
        
        // Apply client authentication middleware to all routes
        .layer(axum::middleware::from_fn_with_state(state.clone(), client_auth))
}