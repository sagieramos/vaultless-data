use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use sqlx::PgPool;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub database: DatabaseHealth,
}

#[derive(Serialize)]
pub struct DatabaseHealth {
    pub connected: bool,
    pub pool_size: u32,
}

/// Health check endpoint
/// GET /health
pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let db_health = check_database(&state.db).await;

    let status = if db_health.connected {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let response = HealthResponse {
        status: if db_health.connected {
            "healthy".to_string()
        } else {
            "unhealthy".to_string()
        },
        version: vaultless_core::VERSION.to_string(),
        database: db_health,
    };

    (status, Json(response))
}

/// Check database connectivity
async fn check_database(pool: &PgPool) -> DatabaseHealth {
    let connected = sqlx::query("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok();

    DatabaseHealth {
        connected,
        pool_size: pool.size(),
    }
}

/// Readiness check endpoint (Kubernetes-friendly)
/// GET /ready
pub async fn readiness_check(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").fetch_one(&state.db).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Liveness check endpoint (Kubernetes-friendly)
/// GET /live
pub async fn liveness_check() -> StatusCode {
    StatusCode::OK
}