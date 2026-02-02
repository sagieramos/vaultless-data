use crate::{handlers::developer::pricing::plan, middleware::user::user_auth, state::AppState};
use axum::{Router, middleware};

pub fn pricing_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/plans", axum::routing::post(plan::create_pricing_plan))
        .route("/plans", axum::routing::get(plan::get_pricing_plans))
        .route(
            "/plans/{plan_id}",
            axum::routing::get(plan::get_pricing_plan),
        )
        .route(
            "/plans/{plan_id}",
            axum::routing::delete(plan::delete_pricing_plan),
        )
        .layer(middleware::from_fn_with_state(state.clone(), user_auth))
}
