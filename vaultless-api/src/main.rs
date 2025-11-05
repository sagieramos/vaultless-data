// api/src/main.rs
use axum::extract::State;
use axum::{Router, middleware as axum_middleware, routing::get};
use deadpool_redis::Config as RedisConfig;
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, signal};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod handlers;
mod middleware;
mod routes;
mod services;
mod state;

use crate::config::Config;
use crate::middleware::track_metrics;
use crate::routes::build_routes;
use crate::state::AppState;

/// Initialize tracing/logging
fn init_tracing(level: &str) {
    let level = level.parse().unwrap_or(tracing::Level::INFO);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()))
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    //----------------------------------------------------
    // 1. Load configuration
    //----------------------------------------------------
    let config = Config::from_env()?;
    init_tracing(&config.server.log_level);

    tracing::info!("🚀 Starting Vaultless API...");

    //----------------------------------------------------
    // 2. Database setup
    //----------------------------------------------------
    tracing::info!("⏳ Connecting to PostgreSQL...");
    let db = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&config.database.url)
        .await?;

    tracing::info!("✅ Connected to PostgreSQL");

    //----------------------------------------------------
    // 3. Redis setup
    //----------------------------------------------------
    tracing::info!("⏳ Connecting to Redis...");
    let redis_cfg = RedisConfig::from_url(config.cache.url.clone());
    let redis_pool = redis_cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;
    tracing::info!("✅ Connected to Redis");

    //----------------------------------------------------
    // 4. Build AppState
    //----------------------------------------------------
    let app_state = AppState::new(db.clone(), redis_pool.clone(), config.clone())?;

    //----------------------------------------------------
    // 5. Routers
    //----------------------------------------------------
    let api_router = build_routes(app_state.clone());
    let metrics_router = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(app_state.clone());

    //----------------------------------------------------
    // 6. Build main app with middleware
    //----------------------------------------------------
    let app = Router::new()
        .merge(api_router)
        .merge(metrics_router)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(CompressionLayer::new())
        .layer(axum_middleware::from_fn(track_metrics))
        .layer(TraceLayer::new_for_http())
        .into_make_service_with_connect_info::<std::net::SocketAddr>();

    //----------------------------------------------------
    // 7. Start server
    //----------------------------------------------------
    let bind_addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid bind address");

    let listener = TcpListener::bind(bind_addr).await?;
    tracing::info!("🌍 Listening on http://{}", listener.local_addr()?);
    tracing::info!(
        "📊 Metrics available at http://{}/metrics",
        listener.local_addr()?
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

//----------------------------------------------------
// Metrics endpoint (Prometheus scrape target)
//----------------------------------------------------
async fn metrics_handler(State(_state): State<AppState>) -> impl axum::response::IntoResponse {
    use prometheus::{Encoder, TextEncoder};

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();

    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
        tracing::error!("Failed to encode metrics: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to encode metrics: {}", e),
        );
    }

    match String::from_utf8(buffer) {
        Ok(metrics_text) => {
            tracing::debug!("📊 Metrics response: {} bytes", metrics_text.len());
            (axum::http::StatusCode::OK, metrics_text)
        }
        Err(e) => {
            tracing::error!("Failed to convert metrics to UTF-8: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to convert metrics: {}", e),
            )
        }
    }
}

//----------------------------------------------------
// Graceful shutdown signal
//----------------------------------------------------
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install signal handler");
        sigterm.recv().await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::warn!("🛑 Shutdown signal received, shutting down gracefully...");
}
/* ```

## Key Features:

1. **Automatic metric registration** using `Lazy` - metrics are registered on first use
2. **Path normalization** to prevent cardinality explosion (e.g., `/users/123` → `/users/:id`)
3. **Multiple metrics**:
   - `http_requests_total` - Request counter by method, path, and status
   - `http_request_duration_seconds` - Request duration histogram
   - `db_query_duration_seconds` - Database query timing (for future use)
   - `cache_operations_total` - Cache operation counter (for future use)
4. **Proper histogram buckets** for latency tracking
5. **Debug logging** for request completion

## Expected Metrics Output:
```
# HELP http_requests_total Number of HTTP requests received
# TYPE http_requests_total counter
http_requests_total{method="GET",path="/health",status="200"} 5
http_requests_total{method="GET",path="/metrics",status="200"} 10
http_requests_total{method="GET",path="/users/:id",status="200"} 15

# HELP http_request_duration_seconds HTTP request duration in seconds
# TYPE http_request_duration_seconds histogram
http_request_duration_seconds_bucket{method="GET",path="/health",le="0.005"} 5
http_request_duration_seconds_sum{method="GET",path="/health"} 0.015
http_request_duration_seconds_count{method="GET",path="/health"} 5 */
