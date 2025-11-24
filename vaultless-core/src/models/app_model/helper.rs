use sqlx::{Executor, Postgres};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::time::{Duration, sleep};

// Global flag to prevent concurrent refreshes
static REFRESH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

pub fn trigger_view_refresh_debounced<'c, E>(exec: E)
where
    E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
{
    // Only trigger if no refresh is currently running
    if REFRESH_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        tokio::spawn(async move {
            // Small delay to batch rapid changes
            sleep(Duration::from_millis(100)).await;

            match refresh_applications_view(exec).await {
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
pub fn trigger_view_refresh<'c, E>(exec: E)
where
    E: Executor<'c, Database = Postgres> + Clone + Send + 'static,
{
    tokio::spawn(async move {
        match refresh_applications_view(exec).await {
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
async fn refresh_applications_view<'c, E>(exec: E) -> sqlx::Result<()>
where
    E: Executor<'c, Database = Postgres>,
{
    sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY mv_applications_with_keys")
        .execute(exec)
        .await?;

    Ok(())
}

/// Synchronous version - use this when you MUST ensure the view is refreshed
/// before continuing (e.g., in tests or critical operations).
///
/// Warning: This will block until the refresh completes.
pub async fn refresh_view_sync<'c, E>(exec: E) -> sqlx::Result<()>
where
    E: Executor<'c, Database = Postgres>,
{
    refresh_applications_view(exec).await
}
