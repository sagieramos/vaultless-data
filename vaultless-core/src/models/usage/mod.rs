pub mod usage;
pub mod usage_timescale;
use std::sync::Arc;
use sqlx::PgPool;
use tokio::task;
use tracing::info;

use usage::*;
use usage_timescale::*;


/* 
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize DB and Redis
    let pg_pool = Arc::new(sqlx::PgPool::connect("postgres://...").await?);
    let redis_pool = Arc::new(create_redis_pool("redis://localhost:6379")?);

    // Start background flusher
    let (flusher_handle, flusher_shutdown) =
        message::init_usage_metrics("redis://localhost:6379", pg_pool.clone()).await;

    // ... start HTTP server, etc.

    // On shutdown:
    flusher_shutdown.notify_one();
    flusher_handle.await?;

    Ok(())
} */
pub async fn init_usage_metrics(
    redis_url: &str,
    pg_pool: Arc<PgPool>,
) -> (task::JoinHandle<()>, Arc<tokio::sync::Notify>) {
    // Create a single Redis connection for background flusher
    let redis_conn = Arc::new(create_redis_conn(redis_url).await.unwrap());

    // Use default config or customize
    let config = MetricsConfig::default();

    // Optional metrics collector
    let flusher_metrics = Arc::new(FlusherMetrics::default());

    // Start flusher task
    let (flusher_handle, shutdown) = start_redis_flusher(
        redis_conn,
        pg_pool.clone(),
        config,
        Some(flusher_metrics.clone()),
    );

    info!("✅ Redis flusher background task started");

    (flusher_handle, shutdown)
}