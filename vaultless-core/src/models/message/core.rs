//! Core InstantMessage implementation - constructor, health status, and metrics helpers.

use super::dto::*;
use super::helper::*;
use crate::circuit_breaker::CircuitBreaker;
use crate::error::{Result, VaultlessError};
use crate::models::usage::{record_message_received, RecordMessageReceivedInput, MetricsConfig};
use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::warn;
use uuid::Uuid;

impl InstantMessage {
    /// Creates a new InstantMessage service with background workers.
    pub fn new(redis_pool: RedisPool, db_pool: PgPool, config: Arc<MetricsConfig>) -> Result<Self> {
        let db_pool_arc = Arc::new(db_pool);
        let weak_db_pool = Arc::downgrade(&db_pool_arc);

        let (tx, rx) = mpsc::channel(CHANNEL_BUFFER);
        let (delete_tx, delete_rx) = mpsc::channel(DELETE_CHANNEL_BUFFER);
        let (dlq_tx, dlq_rx) = mpsc::channel(10_000);

        let metrics = Arc::new(SystemMetrics::new());

        // Circuit breakers: 5 failures within 30 seconds trips the breaker
        let redis_breaker = Arc::new(CircuitBreaker::new(5, 30));
        let db_breaker = Arc::new(CircuitBreaker::new(5, 30));

        let this = Self {
            redis_pool: Arc::new(redis_pool),
            db_pool: db_pool_arc,
            weak_db_pool,
            config,
            sender: tx,
            delete_sender: delete_tx,
            dlq_sender: dlq_tx,
            metrics,
            redis_breaker,
            db_breaker,
        };

        this.spawn_flusher(rx);
        this.spawn_deleter(delete_rx);
        this.spawn_dlq_processor(dlq_rx);
        this.spawn_purger();
        this.spawn_metrics_reporter();

        Ok(this)
    }

    /// Returns health status with channel capacities and circuit breaker states.
    pub fn get_health_status(&self) -> HealthStatus {
        let metrics = self.metrics.get_snapshot();

        HealthStatus {
            flusher_channel_capacity: self.sender.capacity(),
            flusher_channel_available: self.sender.max_capacity() - self.sender.capacity(),
            deleter_channel_capacity: self.delete_sender.capacity(),
            deleter_channel_available: self.delete_sender.max_capacity()
                - self.delete_sender.capacity(),
            dlq_channel_capacity: self.dlq_sender.capacity(),
            dlq_channel_available: self.dlq_sender.max_capacity() - self.dlq_sender.capacity(),
            failed_verifications: metrics.failed_verifications,
            failed_metrics_increments: metrics.failed_metrics_increments,
            emergency_writes: metrics.emergency_writes,
            dlq_entries: metrics.dlq_entries,
            db_pool_dropped_deletes: metrics.db_pool_dropped_deletes,
            db_pool_available: self.weak_db_pool.upgrade().is_some(),
            redis_circuit_state: format!("{:?}", self.redis_breaker.get_state()),
            db_circuit_state: format!("{:?}", self.db_breaker.get_state()),
        }
    }

    // =========================================================================
    // Metrics Helpers with Retry
    // =========================================================================

    /// Retry wrapper for metrics increment with exponential backoff.
    pub async fn increment_received_metrics_with_retry(
        &self,
        application_id: Uuid,
        bytes: i64,
    ) -> Result<()> {
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 50;

        for attempt in 0..MAX_RETRIES {
            match record_message_received(
                &self.redis_pool,
                RecordMessageReceivedInput::new(
                    Uuid::new_v4(), // Use a placeholder message_id for rate limit tracking
                    application_id,
                    String::new(), // Empty session_id for app-only metrics
                    bytes,
                ),
                None,
            )
            .await
            {
                Ok(_) => return Ok(()),
                Err(e) => {
                    warn!(
                        application_id = %application_id,
                        attempt = attempt + 1,
                        error = %e,
                        "Metrics increment failed, retrying"
                    );
                    if attempt < MAX_RETRIES - 1 {
                        use rand::Rng;
                        let delay = BASE_DELAY_MS * (1 << attempt);
                        let jitter = rand::rng().random_range(0..delay / 2);
                        tokio::time::sleep(Duration::from_millis(delay + jitter)).await;
                    }
                }
            }
        }

        Err(VaultlessError::MetricsIncrementFailed(
            "Failed after retries".into(),
        ))
    }
}
