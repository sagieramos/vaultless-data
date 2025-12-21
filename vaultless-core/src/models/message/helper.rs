use super::dto::*;
use crate::cache_key;
use crate::crypto::verify_signature;
use crate::error::Result;
use crate::models::usage::{MetricsConfig, increment_proof_verified_pool};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use redis::RedisResult;
use redis::{AsyncCommands, pipe};
use sqlx::{PgPool, Postgres, query_as, query_builder::QueryBuilder};
use tracing::{error, info};
use uuid::Uuid;

// =============================================================================
// Configuration
// =============================================================================
pub const CACHE_TTL_SECS: u64 = 600;
/// Flush interval in seconds for message batching to DB.
pub const FLUSH_INTERVAL_SECS: u64 = 60;
pub const MAX_BATCH_SIZE: usize = 2000;
pub const CHANNEL_BUFFER: usize = 20_000;
pub const MAX_QUEUE_LEN: isize = 10_000;
pub const CLEANUP_INTERVAL_SECS: u64 = 10;
pub const MAX_DELETE_BATCH: usize = 1000;
pub const DELETE_CHANNEL_BUFFER: usize = 10_000;
pub const PURGE_INTERVAL_HOURS: u64 = 1;
pub const RETENTION_AFTER_DELIVERY_HOURS: i64 = 24;
/// Default message expiry in days.
pub const DEFAULT_MESSAGE_EXPIRY_DAYS: i64 = 7;
pub const MAX_INBOX_FETCH: usize = 100;
pub const PIPELINE_CHUNK_SIZE: usize = 500;
pub const SQL_FALLBACK_PARALLELISM: usize = 4;
// Lock and TTL constants
pub const REBUILD_LOCK_TTL_SECS: i64 = 5;
pub const SENT_COUNTED_TTL_SECS: i64 = 86400; // 24 hours

pub fn instant_message_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "message", msg_id)
}

pub fn instant_inbox_key(client_id: Uuid) -> String {
    cache_key!("instant_message", "inbox", client_id)
}

pub fn instant_pending_read_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "pending_read", msg_id)
}

pub fn instant_rebuild_lock_key(client_id: Uuid) -> String {
    cache_key!("instant_message", "rebuild_lock", client_id)
}

pub fn instant_sent_counted_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "sent_counted", msg_id)
}

pub fn instant_delivered_counted_key(msg_id: Uuid) -> String {
    cache_key!("instant_message", "delivered_counted", msg_id)
}

// =============================================================================
// IoT Redis Keys - optimized for hot paths
// =============================================================================
/// IoT client presence key - tracks if device is online (short TTL, heartbeat refresh)
pub fn iot_presence_key(client_id: Uuid) -> String {
    cache_key!("iot", "presence", client_id)
}

/// IoT telemetry key - stores latest telemetry from device (replaces previous)
pub fn iot_telemetry_key(device_client_id: Uuid) -> String {
    cache_key!("iot", "telemetry", device_client_id)
}

/// IoT command key - stores pending command for device (only if online)
pub fn iot_command_key(device_client_id: Uuid) -> String {
    cache_key!("iot", "command", device_client_id)
}

/// IoT command lock key - prevents duplicate command delivery
pub fn iot_command_lock_key(device_client_id: Uuid) -> String {
    cache_key!("iot", "cmd_lock", device_client_id)
}

// IoT TTL constants
pub const IOT_PRESENCE_TTL_SECS: u64 = 30; // Device must heartbeat within 30s
pub const IOT_TELEMETRY_TTL_SECS: u64 = 300; // Telemetry expires after 5 min
pub const IOT_COMMAND_TTL_SECS: u64 = 60; // Commands expire after 1 min if not fetched

// =============================================================================
// Static soft verify (for parallel fallback) - conditional on require_proof_verification
// =============================================================================
/// Static envelope verification for SQL fallback (no self access).
/// NOTE: This is an async function to allow for the metrics increment call.
pub async fn verify_envelope_soft_static(
    msg: &Message,
    redis_pool: &RedisPool, // Passed in for metrics connection
    config: &MetricsConfig, // Passed in for metrics configuration
) -> bool {
    // 1. Check if verification is required
    let Some(signature_str) = msg.signature.as_deref() else {
        error!("Message signature is missing but required for verification.");
        return false;
    };
    if !msg.require_proof_verification || signature_str.is_empty() {
        return true;
    }
    // 2. Build the envelope struct for serialization
    let envelope = Envelope {
        id: &msg.id,
        sender_client_id: &msg.sender_client_id,
        recipient_client_id: &msg.recipient_client_id,
        application_id: &msg.application_id,
        is_group_message: msg.is_group_message,
        content_size_bytes: msg.content_size_bytes as i64,
        created_at: &msg.created_at,
        require_proof_verification: msg.require_proof_verification,
    };
    // 3. Serialize and verify the signature
    if let Ok(bytes) = serde_json::to_vec(&envelope) {
        if verify_signature(&bytes, signature_str, &msg.envelope_public_key).is_ok() {
            // Signature SUCCESSFUL. Call the proof verified metrics function.
            if let Err(e) =
                increment_proof_verified_pool(redis_pool, msg.application_id, config).await
            {
                // Log the metrics failure, but the core verification is still valid.
                error!(
                    msg_id = %msg.id,
                    application_id = %msg.application_id,
                    error = %e,
                    "Failed to increment proof verified metrics during static verification"
                );
            }
            // Return the core verification result (SUCCESS)
            true
        } else {
            // Signature verification failed.
            false
        }
    } else {
        // Serialization failed.
        false
    }
}
// =============================================================================
// SQL Fallback
// =============================================================================
/// Fetches messages from DB for cache misses.
pub async fn fetch_sql_fallback(
    db_pool: &PgPool,
    ids: &[Uuid],
    recipient: Uuid,
) -> Result<Vec<Message>> {
    query_as(
        "SELECT * FROM messages WHERE id = ANY($1::uuid[]) AND recipient_client_id = $2 AND is_delivered = false",
    )
    .bind(ids)
    .bind(recipient)
    .fetch_all(db_pool)
    .await
    .map_err(Into::into)
}
// =============================================================================
// Emergency Write (for channel backpressure)
// =============================================================================
/// Emergency DB insert for flusher backpressure.
pub async fn emergency_write_message(db_pool: &PgPool, msg: &Message) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO messages (
          id, ciphertext, nonce, content_type, content_size_bytes,
          application_id, created_at, expires_at, access_count,
          is_delivered, delivered_at, max_access_count,
          require_proof_verification, sender_client_id, recipient_client_id,
          group_id, is_group_message
        ) VALUES (
          $1, $2, $3, COALESCE($4, 'application/octet-stream'), $5,
          $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17
        ) ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(msg.id)
    .bind(&msg.ciphertext)
    .bind(msg.nonce)
    .bind(msg.content_type.as_ref()) // Defaults in SQL if None
    .bind(msg.content_size_bytes)
    .bind(msg.application_id)
    .bind(msg.created_at)
    .bind(msg.expires_at)
    .bind(msg.access_count)
    .bind(msg.is_delivered)
    .bind(msg.delivered_at)
    .bind(msg.max_access_count)
    .bind(msg.require_proof_verification)
    .bind(msg.sender_client_id)
    .bind(msg.recipient_client_id)
    .bind(msg.group_id)
    .bind(msg.is_group_message)
    .execute(db_pool)
    .await?;
    Ok(())
}
// =============================================================================
// Batch Insert + Pending Reads + Metrics
// =============================================================================
/// Flushes batch of messages to DB, processes pending reads, cleans Redis.
pub async fn flush_batch(
    db_pool: &PgPool,
    redis_pool: &RedisPool,
    buffer: &mut Vec<Message>,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let start = Utc::now();
    let to_insert = std::mem::take(buffer);
    let mut tx = db_pool.begin().await?;
    let mut qb = QueryBuilder::<Postgres>::new(
        r#"
        INSERT INTO messages (
          id, ciphertext, nonce, content_type, content_size_bytes,
          application_id, created_at, expires_at, access_count,
          is_delivered, delivered_at, max_access_count,
          require_proof_verification, sender_client_id, recipient_client_id,
          group_id, is_group_message
        )
        "#,
    );
    qb.push_values(&to_insert, |mut b, msg| {
        let content_type_str = msg
            .content_type
            .as_deref() // Convert Option<String> to Option<&str>
            .unwrap_or("application/octet-stream");
        b.push_bind(msg.id)
            .push_bind(&msg.ciphertext)
            .push_bind(msg.nonce)
            .push_bind(content_type_str) // Default
            .push_bind(msg.content_size_bytes)
            .push_bind(msg.application_id)
            .push_bind(msg.created_at)
            .push_bind(msg.expires_at)
            .push_bind(msg.access_count)
            .push_bind(msg.is_delivered)
            .push_bind(msg.delivered_at)
            .push_bind(msg.max_access_count)
            .push_bind(msg.require_proof_verification)
            .push_bind(msg.sender_client_id)
            .push_bind(msg.recipient_client_id)
            .push_bind(msg.group_id)
            .push_bind(msg.is_group_message);
    });
    qb.push(" ON CONFLICT (id) DO NOTHING");
    qb.build().execute(&mut *tx).await?;
    tx.commit().await?;
    // Flush pending reads
    let mut rconn = redis_pool.get().await?;
    for msg in &to_insert {
        let pending_key = instant_pending_read_key(msg.id);
        if let Ok(Some(data)) = rconn.get_del::<_, Option<String>>(&pending_key).await
            && let Ok(pending) = serde_json::from_str::<PendingRead>(&data)
        {
            let _ = sqlx::query(
                r#"
                    INSERT INTO p2p_read_receipts (id, message_id, client_id, read_at)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT DO NOTHING
                    "#,
            )
            .bind(Uuid::new_v4())
            .bind(msg.id)
            .bind(pending.reader_client_id)
            .bind(pending.read_at)
            .execute(db_pool)
            .await;
        }
    }
    // Clean Redis (non-DB fields like signature stay in Redis until DEL)
    let mut pipe = pipe();
    for msg in &to_insert {
        pipe.del(instant_message_key(msg.id));
        pipe.lrem(
            instant_inbox_key(msg.recipient_client_id),
            1,
            msg.id.to_string(),
        );
    }
    let _: RedisResult<()> = pipe.query_async(&mut rconn).await;
    let duration_ms = (Utc::now() - start).num_milliseconds();
    info!(
        count = to_insert.len(),
        duration_ms = duration_ms,
        "Flushed messages to database"
    );
    Ok(())
}
// =============================================================================
// Batch Delete
// =============================================================================
/// Processes batch of delete tasks: cleans Redis, deletes/updates DB.
pub async fn delete_batch(
    db_pool: &PgPool,
    redis_pool: &RedisPool,
    buffer: &mut Vec<DeleteTask>,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    let mut rconn = redis_pool.get().await?;
    let mut tx = db_pool.begin().await?;
    let mut pipe = pipe();
    let mut p2p_deletes = Vec::new();
    let mut group_updates = Vec::new();
    let task_count = buffer.len();
    for task in buffer.drain(..) {
        let redis_key = instant_message_key(task.msg_id);
        pipe.del(redis_key);
        if task.is_group_message {
            group_updates.push(task.msg_id);
        } else {
            p2p_deletes.push(task.msg_id);
        }
    }
    let _: () = pipe.query_async(&mut rconn).await?;
    if !p2p_deletes.is_empty() {
        sqlx::query("DELETE FROM messages WHERE id = ANY($1::uuid[]) AND is_group_message = false")
            .bind(&p2p_deletes)
            .execute(&mut *tx)
            .await?;
    }
    if !group_updates.is_empty() {
        sqlx::query(
            r#"
            UPDATE messages
            SET is_delivered = true, delivered_at = $1
            WHERE id = ANY($2::uuid[])
              AND is_group_message = true
              AND delivered_at IS NULL
            "#,
        )
        .bind(Utc::now())
        .bind(&group_updates)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    info!(
        count = task_count,
        p2p = p2p_deletes.len(),
        group = group_updates.len(),
        "Processed delete batch"
    );
    Ok(())
}
