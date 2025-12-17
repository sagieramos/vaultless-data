// vaultless-api/src/main.rs
//! Vaultless API Server
//!
//! Privacy-first, end-to-end encrypted message relay platform.

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
use vaultless_core::models::usage::{FlusherMetrics, MetricsConfig as CoreMetricsConfig, start_redis_flusher};

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

// =============================================================================
// INITIALIZATION
// =============================================================================

/// Initialize tracing/logging
fn init_tracing(level: &str) {
    let level = level.parse().unwrap_or(tracing::Level::INFO);
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()))
        .init();
}

// =============================================================================
// MAIN
// =============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ─────────────────────────────────────────────────────────────────────────
    // 1. Load Configuration
    // ─────────────────────────────────────────────────────────────────────────
    let config = Config::from_env()?;
    init_tracing(&config.server.log_level);

    tracing::info!("🚀 Starting Vaultless API...");
    tracing::debug!(?config, "Loaded configuration");

    // ─────────────────────────────────────────────────────────────────────────
    // 2. Database Connection (PostgreSQL)
    // ─────────────────────────────────────────────────────────────────────────
    tracing::info!(
        host = %config.database.host,
        port = %config.database.port,
        database = %config.database.name,
        "⏳ Connecting to PostgreSQL..."
    );

    let db = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&config.database.connection_url())
        .await?;

    tracing::info!("✅ Connected to PostgreSQL");

    // ─────────────────────────────────────────────────────────────────────────
    // 3. Cache Connection (Redis/Dragonfly)
    // ─────────────────────────────────────────────────────────────────────────
    tracing::info!(
        host = %config.cache.host,
        port = %config.cache.port,
        "⏳ Connecting to Redis/Dragonfly..."
    );

    let redis_cfg = RedisConfig::from_url(config.cache.connection_url());
    let redis_pool = redis_cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))?;

    tracing::info!("✅ Connected to Redis/Dragonfly");

    // ─────────────────────────────────────────────────────────────────────────
    // 4. Build Application State
    // ─────────────────────────────────────────────────────────────────────────
    let metrics_config = Arc::new(CoreMetricsConfig {
        max_batch_size: config.metrics.max_batch_size,
        metric_ttl_secs: config.metrics.ttl_secs,
        flush_interval_secs: config.metrics.flush_interval_secs,
        redis_operation_timeout_secs: config.metrics.redis_timeout_secs,
    });

    let cache_url = config.cache.connection_url();
    let app_state = AppState::new(
        db,
        redis_pool,
        Arc::clone(&metrics_config),
        cache_url,
        config.security.session_key_manager.clone(),
    )?;

    // ─────────────────────────────────────────────────────────────────────────
    // 5. Start Background Services
    // ─────────────────────────────────────────────────────────────────────────
    let flusher_metrics = Arc::new(FlusherMetrics::new());

    let (flusher_handle, shutdown_notify) = start_redis_flusher(
        app_state.redis_pool.clone(),
        app_state.db.clone(),
        Arc::clone(&metrics_config),
        Some(Arc::clone(&flusher_metrics)),
    );

    tracing::info!("✅ Redis metrics flusher started");

    // ─────────────────────────────────────────────────────────────────────────
    // 6. Build Routes
    // ─────────────────────────────────────────────────────────────────────────
    let api_router = build_routes(app_state.clone());
    let metrics_router = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(app_state.clone());

    // ─────────────────────────────────────────────────────────────────────────
    // 7. Build Application with Middleware
    // ─────────────────────────────────────────────────────────────────────────
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

    // ─────────────────────────────────────────────────────────────────────────
    // 8. Start Server
    // ─────────────────────────────────────────────────────────────────────────
    let bind_addr: SocketAddr = config
        .bind_address()
        .parse()
        .expect("Invalid bind address");

    let listener = TcpListener::bind(bind_addr).await?;

    tracing::info!("🌍 Listening on http://{}", listener.local_addr()?);
    tracing::info!(
        "📊 Metrics available at http://{}/metrics",
        listener.local_addr()?
    );
    tracing::info!(
        "📚 API docs available at http://{}/swagger-ui/",
        listener.local_addr()?
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_notify, flusher_handle))
        .await?;

    Ok(())
}

// =============================================================================
// METRICS ENDPOINT
// =============================================================================

/// Prometheus metrics scrape endpoint
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

// =============================================================================
// GRACEFUL SHUTDOWN
// =============================================================================

/// Handle shutdown signals (SIGTERM, SIGINT)
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

    // Signal flusher to stop
    flusher_shutdown.notify_one();

    // Wait for flusher to complete final flush (with timeout)
    if let Err(e) = tokio::time::timeout(Duration::from_secs(30), flusher_handle).await {
        tracing::warn!("Flusher did not complete within 30s: {:?}", e);
    } else {
        tracing::info!("✅ Redis flusher shutdown complete");
    }

    tracing::info!("👋 Goodbye!");
}
