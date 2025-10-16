mod config;
mod handlers;
mod middleware;
mod routes;
mod services;
mod state;

use anyhow::Context;
use axum::middleware as axum_middleware;
use deadpool_redis::{Config as RedisConfig, Runtime};
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    config::Config,
    middleware::{add_request_id, log_request},
    routes::create_router,
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration
    let config = Config::from_env().context("Failed to load configuration")?;

    // Initialize logging
    init_logging(&config);

    tracing::info!("🚀 Starting Vaultless Data API Server");
    tracing::info!("📝 Version: {}", vaultless_core::VERSION);
    tracing::info!(
        "🔧 Environment: {}",
        if cfg!(debug_assertions) {
            "development"
        } else {
            "production"
        }
    );

    // Connect to database
    tracing::info!("📦 Connecting to database...");
    let db_pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
        .context("Failed to connect to database")?;

    tracing::info!("✅ Database connected");

    // Run migrations
    tracing::info!("🔄 Running database migrations...");
    sqlx::migrate!("./migrations")
        .run(&db_pool)
        .await
        .context("Failed to run database migrations")?;

    tracing::info!("✅ Migrations completed");

    // Connect to Dragonfly/Redis cache
    tracing::info!("💾 Connecting to cache...");
    let cache_config = RedisConfig::from_url(&config.cache.url);
    let cache_pool = cache_config
        .create_pool(Some(Runtime::Tokio1))
        .context("Failed to create cache pool")?;

    // Test cache connection
    {
        use deadpool_redis::redis::AsyncCommands;
        let mut conn = cache_pool
            .get()
            .await
            .context("Failed to get cache connection")?;
        let _: () = conn
            .set("health_check", "ok")
            .await
            .context("Cache health check failed")?;
        let _: String = conn
            .get("health_check")
            .await
            .context("Cache health check failed")?;
    }

    tracing::info!("✅ Cache connected");

    // Create application state
    let state = AppState::new(db_pool, cache_pool.clone(), config.clone());

    // Build router with middleware
    let app = create_router(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(CompressionLayer::new())
        .layer(axum_middleware::from_fn(add_request_id))
        .layer(axum_middleware::from_fn(log_request))
        .layer(TraceLayer::new_for_http())
        // Add ConnectInfo for IP address extraction
        .into_make_service_with_connect_info::<std::net::SocketAddr>();
    let bind_addr = config.bind_address();
    tracing::info!("🌐 Server listening on http://{}", bind_addr);
    tracing::info!("📍 Health check: http://{}/health", bind_addr);

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .context(format!("Failed to bind to {}", bind_addr))?;

    axum::serve(listener, app).await.context("Server error")?;

    Ok(())
}

/// Initialize tracing/logging
fn init_logging(config: &Config) {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.server.log_level.clone().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
