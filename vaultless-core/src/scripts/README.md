-- =============================================================================
-- REDIS LUA SCRIPTS FOR VAULTLESS USAGE METRICS
-- =============================================================================
-- Production-ready scripts for atomic quota/rate-limit validation and usage tracking.
-- Designed for thousands of ops/sec in multi-tenant systems.
-- =============================================================================

== SCRIPT CATALOG ==

1. validate_and_record_limits_v1.lua
   - Atomically checks monthly quota and per-minute rate limits
   - Returns status codes (0=OK, 1=QUOTA_EXCEEDED, 2=RATE_LIMIT_EXCEEDED)
   - 4 keys, O(1) operations

2. increment_usage_v1.lua
   - Atomically increments all usage counters after successful request
   - Idempotent via SET NX EX (safe for retries)
   - 5 keys, O(1) operations

3. record_rate_limit_hit_v1.lua
   - Records rate limit hit for analytics
   - Safe to call independently
   - 2 keys, O(1) operations


== RECOMMENDED REDIS KEY NAMING CONVENTIONS ==

All keys use cache_key! macro pattern: prefix:entity:scope:identifier

Monthly Quota Keys:
  - "quota:app:{app_id}:monthly"      -> Integer, current monthly message count
  - "quota:app:{app_id}:bytes:monthly" -> Integer, current monthly bytes

Per-Key Per-Minute Metrics (Hash):
  - "metric:key:{sk_id}:minute:{yyyy_mm_dd_HH_MM}"
    Fields: messages, bytes, total, ratelimit_hits

Per-App Per-Minute Metrics (Hash):
  - "metric:app:{app_id}:minute:{yyyy_mm_dd_HH_MM}"
    Fields: messages, bytes, total, ratelimit_hits

Idempotency Keys (String with TTL):
  - "counted:msg:{msg_id}"              -> Message counting (1hr TTL)
  - "counted:ratelimit:{msg_id}"        -> Rate limit hit counting (1hr TTL)
  - "counted:proof:{msg_id}"            -> Proof verification counting (1hr TTL)

Example key construction in Rust:
  fn monthly_quota_key(app_id: Uuid) -> String {
      cache_key!("quota", "app", app_id, "monthly")
  }

  fn per_key_minute_key(sk_id: Uuid, minute: &DateTime<Utc>) -> String {
      let minute_key = minute.format("%Y_%m_%d_%H_%M").to_string();
      cache_key!("metric", "key", sk_id, "minute", minute_key)
  }


== RETURN CODE REFERENCE ==

validate_and_record_limits_v1.lua:
  0 = OK - Request allowed, caller should proceed with increment_usage
  1 = QUOTA_EXCEEDED - Return 429 with "MONTHLY_QUOTA_EXCEEDED"
  2 = RATE_LIMIT_EXCEEDED - Return 429 with "RATE_LIMIT_EXCEEDED"

increment_usage_v1.lua:
  Returns array: [monthly_count, monthly_bytes, key_minute_total, app_minute_total, was_counted]
  was_counted: 0 = first time, 1 = duplicate (idempotent skip)

record_rate_limit_hit_v1.lua:
  0 = New hit recorded
  1 = Already recorded (duplicate, ignored)
  2 = Error (key type mismatch)


== ATOMICITY GUARANTEES ==

All scripts guarantee:

1. Single-Round-Trip Atomicity
   - All Redis operations execute in single Lua interpreter instance
   - No intermediate state visible to other clients
   - Either all operations complete or none do

2. Idempotency via SET NX EX
   - Idempotency keys use "SET if Not eXists with Expiration"
   - Only first caller succeeds, retries are safely ignored
   - TTL prevents key accumulation (1 hour default)

3. TTL-once Pattern
   - EXISTS check before EXPIRE
   - Prevents refreshing TTL on every call
   - Ensures metrics eventually expire

4. No Loops Over Unbounded Data
   - All scripts use O(1) operations
   - No KEYS or SCAN patterns that could iterate unbounded
   - Fixed number of keys per call


== RUST INTEGRATION PATTERN ==

=== 1. Script Registration (at startup) ===

use redis::Script;

const VALIDATE_AND_RECORD_LUA: &str = include_str!("scripts/validate_and_record_limits_v1.lua");
const INCREMENT_USAGE_LUA: &str = include_str!("scripts/increment_usage_v1.lua");
const RECORD_RATE_LIMIT_HIT_LUA: &str = include_str!("scripts/record_rate_limit_hit_v1.lua");

// Register scripts once at startup
lazy_static! {
    static ref VALIDATE_SCRIPT: redis::Script = Script::new(VALIDATE_AND_RECORD_LUA);
    static ref INCREMENT_SCRIPT: redis::Script = Script::new(INCREMENT_USAGE_LUA);
    static ref RATE_LIMIT_HIT_SCRIPT: redis::Script = Script::new(RECORD_RATE_LIMIT_HIT_LUA);
}

=== 2. Script Caching (EVALSHA optimization) ===

// Store script SHA in Arc<RwLock<HashMap<String, String>>>
struct ScriptCache {
    sha_cache: RwLock<HashMap<&'static str, String>>,
    connection: Arc<RedisConnection>,
}

impl ScriptCache {
    async fn get_sha(&self, name: &'static str) -> Result<String> {
        // Check cache first
        {
            let cache = self.sha_cache.read().await;
            if let Some(sha) = cache.get(name) {
                return Ok(sha.clone());
            }
        }

        // Load script if not cached
        let sha = match name {
            "validate_and_record_limits_v1" => {
                self.connection.script_load(VALIDATE_AND_RECORD_LUA).await?
            }
            "increment_usage_v1" => {
                self.connection.script_load(INCREMENT_USAGE_LUA).await?
            }
            "record_rate_limit_hit_v1" => {
                self.connection.script_load(RECORD_RATE_LIMIT_HIT_LUA).await?
            }
            _ => return Err("Unknown script".into()),
        };

        // Cache the SHA
        let mut cache = self.sha_cache.write().await;
        cache.insert(name, sha.clone());
        Ok(sha)
    }

    async fn evalsha(
        &self,
        name: &'static str,
        keys: Vec<String>,
        args: Vec<String>,
    ) -> Result<redis::Value> {
        let sha = self.get_sha(name).await?;
        Ok(self.connection.evalsha(&sha, keys, args).await?)
    }
}

=== 3. Usage Example: Validate and Increment ===

async fn process_request(
    pool: &RedisPool,
    app_id: Uuid,
    sk_id: Uuid,
    msg_id: Uuid,
    size_bytes: i64,
    quota_limit: i64,
    rate_limit: i64,
) -> Result<(), RateLimitError> {
    let mut conn = pool.get().await?;

    // Generate keys
    let monthly_quota_key = format!("quota:app:{}:monthly", app_id);
    let per_key_minute_key = format!("metric:key:{}:minute:{}", sk_id, current_minute_key());
    let per_app_minute_key = format!("metric:app:{}:minute:{}", app_id, current_minute_key());
    let ratelimit_hit_key = format!("counted:ratelimit:{}", msg_id);

    // Phase 1: Validate limits
    let status: i64 = VALIDATE_SCRIPT
        .key(&monthly_quota_key)
        .key(&per_key_minute_key)
        .key(&per_app_minute_key)
        .key(&ratelimit_hit_key)
        .arg(quota_limit)
        .arg(rate_limit)
        .arg(3600)  // ratelimit_ttl
        .arg(90)    // per_key_ttl
        .arg(90)    // per_app_ttl
        .arg(3600)  // hit_key_ttl
        .invoke_async(&mut conn)
        .await?;

    match status {
        0 => { /* OK, proceed */ }
        1 => return Err(RateLimitError::QuotaExceeded),
        2 => return Err(RateLimitError::RateLimitExceeded),
        _ => return Err(RateLimitError::Unknown),
    }

    // Phase 2: Increment usage (after successful processing)
    let idempotency_key = format!("counted:msg:{}", msg_id);

    let result: (i64, i64, i64, i64, i64) = INCREMENT_SCRIPT
        .key(&monthly_quota_key)
        .key(&format!("quota:app:{}:bytes:monthly", app_id))
        .key(&per_key_minute_key)
        .key(&per_app_minute_key)
        .key(&idempotency_key)
        .arg(size_bytes)
        .arg(31 * 24 * 60 * 60)  // monthly_quota_ttl
        .arg(31 * 24 * 60 * 60)  // monthly_bytes_ttl
        .arg(90)                  // per_key_ttl
        .arg(90)                  // per_app_ttl
        .arg(3600)                // idempotency_ttl
        .arg("")                  // client_id (placeholder)
        .invoke_async(&mut conn)
        .await?;

    let (monthly, monthly_bytes, _, _, was_counted) = result;
    tracing::debug!(monthly, monthly_bytes, was_counted, "Usage incremented");

    Ok(())
}

=== 4. Error Handling with Script Reload ===

// If EVALSHA fails with NOSCRIPT, reload the script
async fn safe_evalsha(
    script: &Script,
    cache: &ScriptCache,
    keys: &[&str],
    args: &[&str],
) -> Result<redis::Value> {
    loop {
        match cache.evalsha(script.name(), keys, args).await {
            Ok(result) => return Ok(result),
            Err(redis::Error:: NOSCRIPT) => {
                // Script was flushed, reload
                cache.invalidate(script.name()).await;
                // Retry with fresh load
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}


== PERFORMANCE CONSIDERATIONS ==

1. Script SHA Caching
   - Always use EVALSHA after first script load
   - Cache SHA in process-local storage (Arc<RwLock<HashMap>>)
   - Handle NOSCRIPT errors gracefully with automatic reload

2. Connection Pooling
   - Scripts should complete in < 5ms (Redis single-threaded)
   - Pool size should match CPU cores + some headroom
   - Use pipelining for independent script calls

3. Key Design for Sharding
   - For very high throughput, consider key sharding:
     - "quota:app:{shard}:{app_id}:monthly" where shard = app_id.hash() % N
   - Scripts must be adjusted to accept shard count as argument

4. Monitoring
   - Track script execution time histogram
   - Monitor NOSCRIPT error rate (indicates Redis memory pressure)
   - Alert on quota/rate-limit hit rate anomalies


== SCRIPT VERSIONING ==

- Version suffix (_v1) is mandatory in filename
- Breaking changes require new version (_v2, etc.)
- Keep old versions until all production traffic migrates
- Consider version negotiation in client SDKs


== SECURITY CONSIDERATIONS ==

1. Key Naming
   - Use UUIDs, not user-controlled input in key names
   - Prevent key injection via ARGV validation

2. Resource Limits
   - Scripts should complete in < 100ms to avoid blocking
   - All operations are O(1) to prevent memory exhaustion

3. Network Security
   - Redis should be in private network
   - Use Redis ACL for multi-tenant isolation
   - Scripts run with same permissions as connection
