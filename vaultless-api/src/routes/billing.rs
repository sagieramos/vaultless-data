use axum::{
    routing::{get, post},
    Router,
};

use crate::handlers;

pub fn billing_routes() -> Router {
    Router::new()
        .route("/process-usage", post(handlers::billing::process_usage_event))
        .route("/client/:client_id/credits", get(handlers::billing::get_client_credits))
}