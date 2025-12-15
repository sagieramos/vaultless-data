use super::dto::*;
use crate::error::{Result, VaultlessError};

use chrono::Utc;
use redis::AsyncCommands;
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use tracing::{error, info};
use uuid::Uuid;

impl InstantMessage {
    /// Get Redis connection with circuit breaker
    async fn get_redis_conn(&self) -> Result<impl AsyncCommands> {
        let guard = self.redis_breaker.allow_request()?;

        match self.redis_pool.get().await {
            Ok(conn) => {
                guard.success();
                Ok(conn)
            }
            Err(e) => {
                guard.failure();
                Err(e.into())
            }
        }
    }

    /// Execute DB query with circuit breaker
    async fn execute_db_query<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(
            &PgPool,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
    {
        let guard = self.db_breaker.allow_request()?;

        match f(&self.db_pool).await {
            Ok(result) => {
                guard.success();
                Ok(result)
            }
            Err(e) => {
                guard.failure();
                Err(e)
            }
        }
    }

    /// Send message to dead letter queue
    async fn send_to_dlq(
        &self,
        msg_id: Uuid,
        reason: DlqReason,
        retry_count: u32,
        original_data: Option<String>,
    ) {
        self.metrics.dlq_entries.fetch_add(1, Ordering::Relaxed);

        let entry = DlqEntry {
            msg_id,
            reason,
            timestamp: Utc::now(),
            retry_count,
            original_data,
        };

        if let Err(e) = self.dlq_sender.try_send(entry) {
            error!(
                msg_id = %msg_id,
                error = ?e,
                "Failed to send to DLQ - message may be permanently lost"
            );
        }
    }

    /// Process DLQ entries (for manual recovery or retry)
    pub async fn process_dlq_entry(&self, msg_id: Uuid) -> Result<()> {
        // Fetch from DLQ
        let entry: Option<(String, i32, Option<String>)> = sqlx::query_as(
            "SELECT reason, retry_count, original_data FROM message_dlq 
             WHERE msg_id = $1 AND processed_at IS NULL",
        )
        .bind(msg_id)
        .fetch_optional(self.db_pool.as_ref())
        .await?;

        let Some((reason, retry_count, _original_data)) = entry else {
            return Err(VaultlessError::NotFound("DLQ entry not found".into()));
        };

        info!(
            msg_id = %msg_id,
            reason = %reason,
            retry_count = retry_count,
            "Processing DLQ entry"
        );

        // Attempt recovery based on reason
        // (Implementation depends on specific recovery strategy)

        // Mark as processed
        sqlx::query("UPDATE message_dlq SET processed_at = NOW() WHERE msg_id = $1")
            .bind(msg_id)
            .execute(self.db_pool.as_ref())
            .await?;

        Ok(())
    }
}
