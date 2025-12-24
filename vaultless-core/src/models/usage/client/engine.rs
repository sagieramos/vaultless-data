//! Atomic Redis operations for client usage metrics.

use crate::cache_key;
use crate::error::{Result, VaultlessError};
use super::counters::{ClientMetricKey, MetricGranularity};
use chrono::Utc;
use deadpool_redis::Pool as RedisPool;
use uuid::Uuid;

// =============================================================================
// Key Generation
// =============================================================================

/// Generate counted key for idempotency
#[inline]
pub fn counted_key(id: Uuid) -> String {
    cache_key!("counted", "client", id)
}

/// Generate client hourly metric key
#[inline]
pub fn client_hourly_key(application_id: Uuid, client_id: Uuid, hour: chrono::DateTime<Utc>) -> String {
    ClientMetricKey::new(application_id, client_id, hour, MetricGranularity::Hour)
        .expect("Valid client metric key")
        .as_str()
        .to_string()
}

// =============================================================================
// Lua Scripts
// =============================================================================

/// Records a message sent by a client.
/// Atomically increments client-specific counters.
///
/// Keys:
/// 1. Idempotency key (`counted:client:{message_id}`)
/// 2. Client hourly metric hash key
///
/// Args:
/// 1. Idempotency key TTL
/// 2. Hourly metric key TTL
/// 3. Size of the message in bytes
///
/// Returns: 1 if counted, 0 if already counted
const RECORD_CLIENT_MESSAGE_SENT_LUA: &str = r#"
local counted_key = KEYS[1]
local hourly_key = KEYS[2]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])
local size_bytes = tonumber(ARGV[3])

-- Idempotency check
local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

-- Increment client hourly metrics
local initial_creation = (redis.call('EXISTS', hourly_key) == 0)
redis.call('HINCRBY', hourly_key, 'messages_sent', 1)
redis.call('HINCRBY', hourly_key, 'total_bytes_sent', size_bytes)
if initial_creation then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
"#;

/// Records a message received by a client.
const RECORD_CLIENT_MESSAGE_RECEIVED_LUA: &str = r#"
local counted_key = KEYS[1]
local hourly_key = KEYS[2]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])
local size_bytes = tonumber(ARGV[3])

local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

local initial_creation = (redis.call('EXISTS', hourly_key) == 0)
redis.call('HINCRBY', hourly_key, 'messages_received', 1)
redis.call('HINCRBY', hourly_key, 'total_bytes_received', size_bytes)
if initial_creation then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
"#;

/// Records a proof verified by a client.
const RECORD_CLIENT_PROOF_VERIFIED_LUA: &str = r#"
local counted_key = KEYS[1]
local hourly_key = KEYS[2]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])

local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

local initial_creation = (redis.call('EXISTS', hourly_key) == 0)
redis.call('HINCRBY', hourly_key, 'proofs_verified', 1)
if initial_creation then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
"#;

/// Records a rate limit hit for a client.
const RECORD_CLIENT_RATE_LIMIT_HIT_LUA: &str = r#"
local counted_key = KEYS[1]
local hourly_key = KEYS[2]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])

local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

local initial_creation = (redis.call('EXISTS', hourly_key) == 0)
redis.call('HINCRBY', hourly_key, 'rate_limit_hits', 1)
if initial_creation then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
"#;

// =============================================================================
// Configuration
// =============================================================================

static DEFAULT_CONFIG: once_cell::sync::Lazy<ClientEngineConfig> =
    once_cell::sync::Lazy::new(ClientEngineConfig::default);

#[derive(Debug, Clone)]
pub struct ClientEngineConfig {
    pub counted_ttl_secs: i64,
    pub hourly_ttl_secs: i64,
    pub operation_timeout_secs: u64,
}

impl Default for ClientEngineConfig {
    fn default() -> Self {
        Self {
            counted_ttl_secs: 3600, // 1 hour
            hourly_ttl_secs: 7200,  // 2 hours
            operation_timeout_secs: 5,
        }
    }
}

// =============================================================================
// Input Types
// =============================================================================

#[derive(Debug, Clone)]
pub struct RecordClientMessageSentInput {
    pub message_id: Uuid,
    pub application_id: Uuid,
    pub client_id: Uuid,
    pub content_size_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct RecordClientMessageReceivedInput {
    pub message_id: Uuid,
    pub application_id: Uuid,
    pub client_id: Uuid,
    pub content_size_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct RecordClientProofVerifiedInput {
    pub proof_id: Uuid, // Or some unique identifier for the proof event
    pub application_id: Uuid,
    pub client_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct RecordClientRateLimitHitInput {
    pub request_id: Uuid, // Or some unique identifier for the request
    pub application_id: Uuid,
    pub client_id: Uuid,
}

// =============================================================================
// Core Operations
// =============================================================================

async fn run_script(
    pool: &RedisPool,
    script: &str,
    keys: Vec<String>,
    args: Vec<i64>,
    timeout_secs: u64,
    error_context: &str,
) -> Result<bool> {
    let mut conn = pool.get().await.map_err(|e| VaultlessError::Internal(e.to_string()))?;
    let mut script_cmd = redis::Script::new(script);
    for key in keys {
        script_cmd.key(key);
    }
    for arg in args {
        script_cmd.arg(arg);
    }

    let result: i64 = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        script_cmd.invoke_async(&mut conn),
    )
    .await
    .map_err(|_| VaultlessError::Timeout(format!("{} timed out", error_context)))?
    .map_err(|e| VaultlessError::Internal(format!("Lua script error in {}: {}", error_context, e)))?;

    Ok(result == 1)
}


#[inline]
pub async fn record_client_message_sent(
    pool: &RedisPool,
    input: RecordClientMessageSentInput,
    config: Option<&'static ClientEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let now = Utc::now();
    let keys = vec![
        counted_key(input.message_id),
        client_hourly_key(input.application_id, input.client_id, now),
    ];
    let args = vec![
        config.counted_ttl_secs,
        config.hourly_ttl_secs,
        input.content_size_bytes,
    ];
    run_script(pool, RECORD_CLIENT_MESSAGE_SENT_LUA, keys, args, config.operation_timeout_secs, "record_client_message_sent").await
}

#[inline]
pub async fn record_client_message_received(
    pool: &RedisPool,
    input: RecordClientMessageReceivedInput,
    config: Option<&'static ClientEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let now = Utc::now();
    let keys = vec![
        counted_key(input.message_id),
        client_hourly_key(input.application_id, input.client_id, now),
    ];
    let args = vec![
        config.counted_ttl_secs,
        config.hourly_ttl_secs,
        input.content_size_bytes,
    ];
    run_script(pool, RECORD_CLIENT_MESSAGE_RECEIVED_LUA, keys, args, config.operation_timeout_secs, "record_client_message_received").await
}

#[inline]
pub async fn record_client_proof_verified(
    pool: &RedisPool,
    input: RecordClientProofVerifiedInput,
    config: Option<&'static ClientEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let now = Utc::now();
    let keys = vec![
        counted_key(input.proof_id),
        client_hourly_key(input.application_id, input.client_id, now),
    ];
    let args = vec![config.counted_ttl_secs, config.hourly_ttl_secs];
    run_script(pool, RECORD_CLIENT_PROOF_VERIFIED_LUA, keys, args, config.operation_timeout_secs, "record_client_proof_verified").await
}

#[inline]
pub async fn record_client_rate_limit_hit(
    pool: &RedisPool,
    input: RecordClientRateLimitHitInput,
    config: Option<&'static ClientEngineConfig>,
) -> Result<bool> {
    let config = config.unwrap_or(&DEFAULT_CONFIG);
    let now = Utc::now();
    let keys = vec![
        counted_key(input.request_id),
        client_hourly_key(input.application_id, input.client_id, now),
    ];
    let args = vec![config.counted_ttl_secs, config.hourly_ttl_secs];
    run_script(pool, RECORD_CLIENT_RATE_LIMIT_HIT_LUA, keys, args, config.operation_timeout_secs, "record_client_rate_limit_hit").await
}
