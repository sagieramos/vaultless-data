use crate::error::{self, VaultlessError};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::AsyncCommands;
use sqlx::Pool;
use sqlx::Postgres;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::{Duration, sleep};

pub const MV_ETAG_KEY: &str = "mv_applications_etag";

// Global flag to prevent concurrent refreshes
static REFRESH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn trigger_view_refresh_debounced(db: Arc<Pool<Postgres>>, redis: Arc<RedisPool>) {
    // Only trigger if no refresh is currently running
    if REFRESH_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        tokio::spawn(async move {
            // Small delay to batch rapid changes
            sleep(Duration::from_millis(100)).await;

            match refresh_applications_view(&db, redis).await {
                Ok(_) => {
                    tracing::debug!("Successfully refreshed mv_applications_with_keys");
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "Failed to refresh mv_applications_with_keys"
                    );
                }
            }

            // Release the lock
            REFRESH_IN_PROGRESS.store(false, Ordering::SeqCst);
        });
    } else {
        tracing::debug!("View refresh already in progress, skipping");
    }
}

/// Helper to refresh the applications_with_keys materialized view.
///
/// This should be called after any operation that modifies:
/// - Applications (create, update, delete)
/// - API Keys (create, delete, activate/deactivate)
///
/// The refresh is done in the background to avoid blocking the main operation.
pub fn trigger_view_refresh(db: Arc<Pool<Postgres>>, redis: Arc<RedisPool>) {
    tokio::spawn(async move {
        match refresh_applications_view(&db, redis).await {
            Ok(_) => {
                tracing::debug!("Successfully refreshed mv_applications_with_keys");
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Failed to refresh mv_applications_with_keys materialized view"
                );
            }
        }
    });
}

/// Performs the actual refresh operation.
/// Uses CONCURRENTLY to avoid locking the view during reads.
async fn refresh_applications_view(db: &Pool<Postgres>, redis: Arc<RedisPool>) -> sqlx::Result<()> {
    sqlx::query!(r#"REFRESH MATERIALIZED VIEW CONCURRENTLY mv_applications_with_usage"#)
        .execute(db)
        .await?;

    set_global_mv_etag(redis).await;

    Ok(())
}

/// Synchronous version - use this when you MUST ensure the view is refreshed
/// before continuing (e.g., in tests or critical operations).
///
/// Warning: This will block until the refresh completes.
pub async fn refresh_view_sync(db: &Pool<Postgres>, redis: Arc<RedisPool>) -> sqlx::Result<()> {
    refresh_applications_view(db, redis).await
}

pub async fn get_global_mv_etag(redis: &RedisPool) -> error::Result<Option<i64>> {
    let mut conn = redis
        .get()
        .await
        .map_err(|_| VaultlessError::Internal("Failed to get Redis connection".into()))?;
    let v: Option<i64> = conn.get(MV_ETAG_KEY).await?;
    Ok(v)
}

async fn set_global_mv_etag(redis: Arc<RedisPool>) {
    if let Ok(mut conn) = redis.get().await {
        let cache_key = crate::cache_key!(MV_ETAG_KEY);
        let ts = Utc::now().timestamp_millis();
        let _: Result<(), _> = conn.set(&cache_key, ts).await;
        let _: Result<(), _> = conn.expire(cache_key, 3600).await;
    }
}
