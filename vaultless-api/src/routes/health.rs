use axum::{Json, extract::State, http::StatusCode};
use deadpool_redis::{Pool as RedisPool, redis::cmd};
use serde::Serialize;
use sqlx::PgPool;

use tokio::time::{Duration, timeout};

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
    pub pool_size: usize,
}
#[derive(Serialize)]
pub struct CacheHealth {
    pub connected: bool,
    pub pool_size: usize,
    pub available: usize,
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

/// New Axum handler for the /check_cache route
/// GET /check_cache
pub async fn check_cache_handler(State(state): State<AppState>) -> (StatusCode, Json<CacheHealth>) {
    let cache_health = check_cache(&state.redis_pool).await;

    let status = if cache_health.connected {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(cache_health))
}

/// Check DragonflyDB (Redis) connectivity
async fn check_cache(pool: &RedisPool) -> CacheHealth {
    // Avoid waiting too long for a pool connection
    let connected = match timeout(Duration::from_millis(200), pool.get()).await {
        Ok(Ok(mut conn)) => {
            match timeout(
                Duration::from_millis(200),
                cmd("PING").query_async::<String>(&mut conn),
            )
            .await
            {
                Ok(Ok(pong)) => pong == "PONG",
                _ => false,
            }
        }
        _ => false,
    };

    let status = pool.status();

    CacheHealth {
        connected,
        pool_size: status.size,
        available: status.available,
    }
}

/// Check database connectivity
async fn check_database(pool: &PgPool) -> DatabaseHealth {
    let connected = sqlx::query("SELECT 1").fetch_one(pool).await.is_ok();

    DatabaseHealth {
        connected,
        pool_size: pool.size() as usize,
    }
}

/// Readiness check endpoint (Kubernetes-friendly)
/// GET /ready
pub async fn readiness_check(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").fetch_one(state.db.as_ref()).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Liveness check endpoint (Kubernetes-friendly)
/// GET /live
pub async fn liveness_check() -> StatusCode {
    StatusCode::OK
}
