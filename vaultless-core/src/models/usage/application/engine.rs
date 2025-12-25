//! Atomic Redis operations for billing-critical usage metrics.
//!
//! All counter mutations use Lua scripts for true atomicity.
//! Single round-trip for application + client + session metrics.
//!
//! # Key Principles
//!
//! - Idempotency: SET NX EX prevents double-counting
//! - Atomicity: All counters increment or none do
//! - TTL-once: EXPIRE only on first creation
//! - Single round-trip: No partial failures possible
//!
//! # Key Structure
//!
//! - `counted:{msg_id}` - Idempotency key (expires quickly)
//! - `metric:app:{app_id}:monthly:{year_month}` - App monthly quota (hash)
//! - `metric:app:{app_id}:hourly:{hour}` - App hourly metrics (hash)
//! - `metric:session:{session_id}:sent` - Session message count (string)
//! - `metric:session:{session_id}:bytes_sent` - Session bytes sent (string)
//! - `metric:session:{session_id}:received` - Session message received (string)
//! - `metric:session:{session_id}:bytes_received` - Session bytes received (string)
//! - `metric:session:{session_id}:proved` - Session proofs verified (string)

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use crate::models::usage::counters::{MetricGranularity, MetricKey};
use chrono::{Datelike, Utc};
use deadpool_redis::Pool as RedisPool;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::usage::config::UsageEngineConfig;

// =============================================================================
// Lua Scripts (loaded from external files)
// =============================================================================

const RECORD_MESSAGE_SENT_LUA: &str = include_str!("../../../scripts/usage_record_message_sent.lua");
const RECORD_MESSAGE_RECEIVED_LUA: &str = include_str!("../../../scripts/usage_record_message_received.lua");
const RECORD_PROOF_VERIFIED_LUA: &str = include_str!("../../../scripts/usage_record_proof_verified.lua");
const RECORD_RATE_LIMIT_HIT_LUA: &str = include_str!("../../../scripts/usage_record_rate_limit_hit.lua");
const INCREMENT_SESSION_LUA: &str = include_str!("../../../scripts/usage_increment_session.lua");

// =============================================================================
// Key Generation
// =============================================================================

/// Generate counted key for idempotency
#[inline]
pub fn counted_key(message_id: Uuid) -> String {
    cache_key!("counted", message_id)
}

/// Generate application monthly quota key
#[inline]
pub fn app_monthly_key(application_id: Uuid) -> String {
    let now = Utc::now();
    let year_month = format!("{:04}{:02}", now.year(), now.month());
    cache_key!("metric", "app", application_id, "monthly", year_month)
}

/// Generate application hourly metric key
#[inline]
pub fn app_hourly_key(application_id: Uuid, hour: chrono::DateTime<Utc>) -> String {
    MetricKey::new(application_id, hour, MetricGranularity::Hour)
        .expect("Valid metric key")
        .as_str()
        .to_string()
}

/// Generate session metric key
#[inline]
pub fn session_metric_key(session_id: &str, counter_type: &str) -> String {
    cache_key!("metric", "session", session_id, counter_type)
}

// =============================================================================
// Configuration
// =============================================================================

/// Default engine configuration (lazily initialized)
static DEFAULT_CONFIG: once_cell::sync::Lazy<UsageEngineConfig> =
    once_cell::sync::Lazy::new(|| UsageEngineConfig::default());

// =============================================================================
// Input Types
// =============================================================================

/// Input for message sent recording
#[derive(Debug, Clone)]
pub struct RecordMessageSentInput {
    pub message_id: Uuid,
    pub application_id: Uuid,
    pub session_id: String,
    pub content_size_bytes: i64,
}

impl RecordMessageSentInput {
    #[inline]
    pub fn new(message_id: Uuid, application_id: Uuid, session_id: String, content_size_bytes: i64) -> Self {
        Self {
            message_id,
            application_id,
            session_id,
            content_size_bytes,
        }
    }
}

/// Input for message received recording
#[derive(Debug, Clone)]
pub struct RecordMessageReceivedInput {
    pub message_id: Uuid,
    pub application_id: Uuid,
    pub session_id: String,
    pub content_size_bytes: i64,
}

impl RecordMessageReceivedInput {
    #[inline]
    pub fn new(message_id: Uuid, application_id: Uuid, session_id: String, content_size_bytes: i64) -> Self {
        Self {
            message_id,
            application_id,
            session_id,
            content_size_bytes,
        }
    }
}

/// Input for proof verified recording
#[derive(Debug, Clone)]
pub struct RecordProofVerifiedInput {
    pub message_id: Uuid,
    pub application_id: Uuid,
    pub session_id: String,
}

impl RecordProofVerifiedInput {
    #[inline]
    pub fn new(message_id: Uuid, application_id: Uuid, session_id: String) -> Self {
        Self {
            message_id,
            application_id,
            session_id,
        }
    }
}

/// Input for rate limit hit recording
#[derive(Debug, Clone)]
pub struct RecordRateLimitHitInput {
    pub message_id: Uuid,
    pub application_id: Uuid,
}

impl RecordRateLimitHitInput {
    #[inline]
    pub fn new(message_id: Uuid, application_id: Uuid) -> Self {
        Self {
            message_id,
            application_id,
        }
    }
}

// =============================================================================
// Core Operations
// =============================================================================

/// Atomically record a message sent event.
#[inline]
pub async fn record_message_sent(
    pool: &RedisPool,
    input: RecordMessageSentInput,
    config: Option<&'static UsageEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let counted_key = counted_key(input.message_id);
    let monthly_key = app_monthly_key(input.application_id);
    let hourly_key = app_hourly_key(input.application_id, Utc::now());
    let session_sent_key = session_metric_key(&input.session_id, "sent");
    let session_bytes_key = session_metric_key(&input.session_id, "bytes_sent");

    let result: i64 = tokio::time::timeout(
        std::time::Duration::from_secs(config.operation_timeout_secs),
        redis::Script::new(RECORD_MESSAGE_SENT_LUA)
            .key(&counted_key)
            .key(&monthly_key)
            .key(&hourly_key)
            .key(&session_sent_key)
            .key(&session_bytes_key)
            .arg(config.counted_ttl_secs)
            .arg(config.monthly_ttl_secs)
            .arg(config.hourly_ttl_secs)
            .arg(config.session_ttl_secs)
            .arg(input.content_size_bytes)
            .invoke_async(&mut conn),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("record_message_sent timed out".into()))?
    .map_err(|e| VaultlessError::Internal(format!("Lua script error: {}", e)))?;

    Ok(result == 1)
}

/// Atomically record a message received event.
///
/// Returns `Ok(true)` if the message was counted, `Ok(false)` if it was already counted.
#[inline]
pub async fn record_message_received(
    pool: &RedisPool,
    input: RecordMessageReceivedInput,
    config: Option<&'static UsageEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let counted_key = counted_key(input.message_id);
    let hourly_key = app_hourly_key(input.application_id, Utc::now());
    let session_rcvd_key = session_metric_key(&input.session_id, "received");
    let session_bytes_key = session_metric_key(&input.session_id, "bytes_received");

    let result: i64 = tokio::time::timeout(
        std::time::Duration::from_secs(config.operation_timeout_secs),
        redis::Script::new(RECORD_MESSAGE_RECEIVED_LUA)
            .key(&counted_key)
            .key(&hourly_key)
            .key(&session_rcvd_key)
            .key(&session_bytes_key)
            .arg(config.counted_ttl_secs)
            .arg(config.hourly_ttl_secs)
            .arg(config.session_ttl_secs)
            .arg(input.content_size_bytes)
            .invoke_async(&mut conn),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("record_message_received timed out".into()))?
    .map_err(|e| VaultlessError::Internal(format!("Lua script error: {}", e)))?;

    Ok(result == 1)
}

/// Atomically record a proof verified event.
///
/// Returns `Ok(true)` if the proof was counted, `Ok(false)` if it was already counted.
#[inline]
pub async fn record_proof_verified(
    pool: &RedisPool,
    input: RecordProofVerifiedInput,
    config: Option<&'static UsageEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let counted_key = counted_key(input.message_id);
    let hourly_key = app_hourly_key(input.application_id, Utc::now());
    let session_proved_key = session_metric_key(&input.session_id, "proved");

    let result: i64 = tokio::time::timeout(
        std::time::Duration::from_secs(config.operation_timeout_secs),
        redis::Script::new(RECORD_PROOF_VERIFIED_LUA)
            .key(&counted_key)
            .key(&hourly_key)
            .key(&session_proved_key)
            .arg(config.counted_ttl_secs)
            .arg(config.hourly_ttl_secs)
            .arg(config.session_ttl_secs)
            .invoke_async(&mut conn),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("record_proof_verified timed out".into()))?
    .map_err(|e| VaultlessError::Internal(format!("Lua script error: {}", e)))?;

    Ok(result == 1)
}

/// Atomically record a rate limit hit event.
///
/// Returns `Ok(true)` if the hit was counted, `Ok(false)` if it was already counted.
#[inline]
pub async fn record_rate_limit_hit(
    pool: &RedisPool,
    input: RecordRateLimitHitInput,
    config: Option<&'static UsageEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let counted_key = counted_key(input.message_id);
    let hourly_key = app_hourly_key(input.application_id, Utc::now());

    let result: i64 = tokio::time::timeout(
        std::time::Duration::from_secs(config.operation_timeout_secs),
        redis::Script::new(RECORD_RATE_LIMIT_HIT_LUA)
            .key(&counted_key)
            .key(&hourly_key)
            .arg(config.counted_ttl_secs)
            .arg(config.hourly_ttl_secs)
            .invoke_async(&mut conn),
    )
    .await
    .map_err(|_| VaultlessError::Timeout("record_rate_limit_hit timed out".into()))?
    .map_err(|e| VaultlessError::Internal(format!("Lua script error: {}", e)))?;

    Ok(result == 1)
}

// =============================================================================
// Multi-Operation (for batch processing)
// =============================================================================

/// Result of a multi-operation batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiRecordResult {
    pub message_sent_counted: bool,
    pub message_received_counted: bool,
    pub proof_verified_counted: bool,
    pub any_counted: bool,
}

/// Record multiple events in sequence (each still atomic per event).
/// Returns which events were counted.
pub async fn record_message_events(
    pool: &RedisPool,
    sent: Option<RecordMessageSentInput>,
    received: Option<RecordMessageReceivedInput>,
    proved: Option<RecordProofVerifiedInput>,
    config: Option<&'static UsageEngineConfig>,
) -> Result<MultiRecordResult> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);

    let sent_counted = match sent {
        Some(input) => record_message_sent(pool, input, Some(config)).await?,
        None => false,
    };

    let received_counted = match received {
        Some(input) => record_message_received(pool, input, Some(config)).await?,
        None => false,
    };

    let proved_counted = match proved {
        Some(input) => record_proof_verified(pool, input, Some(config)).await?,
        None => false,
    };

    Ok(MultiRecordResult {
        message_sent_counted: sent_counted,
        message_received_counted: received_counted,
        proof_verified_counted: proved_counted,
        any_counted: sent_counted || received_counted || proved_counted,
    })
}

// =============================================================================
// Session-Only Operations (no idempotency, no app metrics)
// =============================================================================

/// Increment session counters without idempotency or app metrics.
/// Use this for background/cached updates where you already know the counts.
#[inline]
pub async fn increment_session_counters(
    pool: &RedisPool,
    session_id: &str,
    sent_delta: i64,
    bytes_sent_delta: i64,
    received_delta: i64,
    bytes_received_delta: i64,
    proved_delta: i64,
    ttl_secs: i64,
) -> Result<()> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let sent_key = session_metric_key(session_id, "sent");
    let bytes_sent_key = session_metric_key(session_id, "bytes_sent");
    let received_key = session_metric_key(session_id, "received");
    let bytes_received_key = session_metric_key(session_id, "bytes_received");
    let proved_key = session_metric_key(session_id, "proved");

    let _: () = redis::Script::new(INCREMENT_SESSION_LUA)
        .key(&sent_key)
        .key(&bytes_sent_key)
        .key(&received_key)
        .key(&bytes_received_key)
        .key(&proved_key)
        .arg(sent_delta)
        .arg(bytes_sent_delta)
        .arg(received_delta)
        .arg(bytes_received_delta)
        .arg(proved_delta)
        .arg(ttl_secs)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| VaultlessError::Internal(format!("Session increment error: {}", e)))?;

    Ok(())
}

/// Get current session counters from Redis
#[inline]
pub async fn get_session_counters(
    pool: &RedisPool,
    session_id: &str,
) -> Result<Option<(i64, i64, i64, i64, i64)>> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    let sent_key = session_metric_key(session_id, "sent");
    let bytes_sent_key = session_metric_key(session_id, "bytes_sent");
    let received_key = session_metric_key(session_id, "received");
    let bytes_received_key = session_metric_key(session_id, "bytes_received");
    let proved_key = session_metric_key(session_id, "proved");

    let (sent, bytes_sent, received, bytes_received, proved): (i64, i64, i64, i64, i64) = redis::pipe()
        .get(&sent_key)
        .get(&bytes_sent_key)
        .get(&received_key)
        .get(&bytes_received_key)
        .get(&proved_key)
        .query_async(&mut conn)
        .await
        .map_err(|e| VaultlessError::Internal(e.to_string()))?;

    if sent == 0 && bytes_sent == 0 && received == 0 && bytes_received == 0 && proved == 0 {
        Ok(None)
    } else {
        Ok(Some((sent, bytes_sent, received, bytes_received, proved)))
    }
}

// =============================================================================
// Why This Design?
// =============================================================================

/*

## Before (Multiple Round-Trips)

```rust
// Idempotency check (1 round-trip)
redis.set(counted_key, "1", "NX", "EX", ttl);

// Monthly quota (1-2 round-trips)
redis.incr(monthly_key);
if first { redis.expire(monthly_key, ttl); }

// Hourly metrics (3 round-trips)
redis.hincr(hourly_key, "messages_sent", 1);
redis.hincr(hourly_key, "total_bytes_sent", size);
if first { redis.expire(hourly_key, ttl); }

// Session metrics (2 round-trips)
redis.incr(session_sent_key);
redis.incrby(session_bytes_key, size);
redis.expire(session_sent_key, session_ttl);
redis.expire(session_bytes_key, session_ttl);
```

Total: 8-12 Redis round-trips, partial failures possible.

## After (Single Atomic Call)

```rust
let counted = record_message_sent(pool, input, config).await?;
```

Total: 1 Redis round-trip, all-or-nothing atomicity.

## What Lua Guarantees

1. **Atomicity**: Redis executes the entire script without interruption
2. **Idempotency**: SET NX EX prevents double-counting
3. **TTL-once**: EXISTS check ensures EXPIRE runs only once
4. **Consistency**: All counters update or none do
5. **No retries**: Unlike WATCH-based transactions, Lua never fails under contention

## Billing Safety

- Message counted exactly once (idempotency key)
- App monthly quota accurate (atomic INCR)
- Client/session analytics consistent (all in one script)
- No lost increments (no partial completion)
- No double-billing (idempotency key expires)

*/
