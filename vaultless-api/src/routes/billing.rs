use axum::{
    Router,
    routing::get,
};

use crate::{
    handlers::developer::billing::*,
    middleware::user::user_auth,
    state::AppState,
};

pub fn billing_routes(state: AppState) -> Router<AppState> {
    Router::new()
        // Billing overview and history
        .route("/overview", get(get_billing_overview))
        .route("/history", get(get_billing_history))

        // Usage and revenue reports
        .route("/usage", get(get_usage_report))
        .route("/revenue", get(get_revenue_report))

        // Current billing period
        .route("/current-period", get(get_current_billing_period))

        // Invoices
        .route("/invoices", get(get_invoices))

        // Apply user authentication middleware to all routes
        .layer(axum::middleware::from_fn_with_state(state.clone(), user_auth))
}