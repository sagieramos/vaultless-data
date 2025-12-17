// api/src/main.rs
use axum::extract::State;
use axum::{Router, middleware as axum_middleware, routing::get};
use deadpool_redis::Config as RedisConfig;
use sqlx::postgres::PgPoolOptions;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, signal};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vaultless_core::models::usage::{FlusherMetrics, MetricsConfig, start_redis_flusher};
mod api_doc;
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

    let metrics_config = Arc::new(MetricsConfig {
        max_batch_size: config.metrics_max_batch_size.unwrap_or(1000),
        metric_ttl_secs: config.metrics_ttl_secs.unwrap_or(7200),
        flush_interval_secs: config.metrics_flush_interval_secs.unwrap_or(60),
        redis_operation_timeout_secs: config.metrics_redis_timeout_secs.unwrap_or(30),
    });

    let app_state = AppState::new(
        db,
        redis_pool,
        Arc::clone(&metrics_config),
        config.cache.url,
        config.security.paseto_client_session_key_manager.clone(),
    )?;

    let flusher_metrics = Arc::new(FlusherMetrics::new());

    let (flusher_handle, shutdown_notify) = start_redis_flusher(
        app_state.redis_pool.clone(),
        app_state.db.clone(),
        Arc::clone(&metrics_config),
        Some(Arc::clone(&flusher_metrics)),
    );

    tracing::info!("✅ Redis metrics flusher started");

    //----------------------------------------------------
    // 5. Routers
    //----------------------------------------------------
    let api_router = build_routes(app_state.clone());
    let metrics_router = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(app_state.clone());

    //----------------------------------------------------
    // 6. Build main app with middleware and Swagger UI
    //----------------------------------------------------
    let swagger_ui = api_doc::openapi_config();

    let app = Router::new()
        .merge(api_router)
        .merge(metrics_router)
        .merge(swagger_ui)
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
        .with_graceful_shutdown(shutdown_signal(shutdown_notify, flusher_handle))
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
async fn shutdown_signal(
    flusher_shutdown: Arc<tokio::sync::Notify>,
    flusher_handle: tokio::task::JoinHandle<()>,
) {
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
    flusher_shutdown.notify_one();

    // Wait for flusher to complete final flush
    if let Err(e) = tokio::time::timeout(std::time::Duration::from_secs(30), flusher_handle).await {
        tracing::warn!("Flusher did not complete within 30s: {:?}", e);
    } else {
        tracing::info!("✅ Redis flusher shutdown complete");
    }
}
