-- =============================================================================
-- validate_and_record_limits_v1.lua
-- =============================================================================
-- Atomically validates and records usage limits for an API request.
-- luacheck: globals KEYS ARGV redis
-- Performs quota and rate limit checks in a single atomic operation.
--
-- KEY STRUCTURE (4 keys, O(1)):
--   KEYS[1] = Monthly quota key for application (e.g., "quota:app:{app_id}:monthly")
--   KEYS[2] = Per-key per-minute metrics key (e.g., "metric:key:{sk_id}:minute:{minute_key}")
--   KEYS[3] = Per-app per-minute metrics key (e.g., "metric:app:{app_id}:minute:{minute_key}")
--   KEYS[4] = Rate limit hit idempotency key (e.g., "counted:ratelimit:{msg_id}")
--
-- ARGUMENTS (6 args):
--   ARGV[1] = Application monthly quota limit (messages)
--   ARGV[2] = API key rate limit per minute
--   ARGV[3] = TTL for rate limit hit key (seconds)
--   ARGV[4] = TTL for per-key metrics (seconds)
--   ARGV[5] = TTL for per-app metrics (seconds)
--   ARGV[6] = Rate limit hit key TTL (seconds)
--
-- RETURN CODES:
--   0 = OK - Request allowed, counters read
--   1 = QUOTA_EXCEEDED - Monthly quota exceeded
--   2 = RATE_LIMIT_EXCEEDED - Per-minute rate limit exceeded
--
-- ATOMICITY GUARANTEES:
--   - All reads and writes happen in single Lua script execution
--   - No intermediate state visible to other clients
--   - SET NX EX ensures idempotent rate limit hit recording
-- =============================================================================

local monthly_quota_key = KEYS[1]
local per_key_minute_key = KEYS[2]
local per_app_minute_key = KEYS[3]
local ratelimit_hit_key = KEYS[4]

local app_monthly_limit = tonumber(ARGV[1])
local key_rate_limit = tonumber(ARGV[2])
local ratelimit_ttl = tonumber(ARGV[3])
local per_key_ttl = tonumber(ARGV[4])
local per_app_ttl = tonumber(ARGV[5])
local hit_key_ttl = tonumber(ARGV[6])

-- Step 1: Read current usage in single atomic call
-- Using pipelined GET for O(1) operation
local current_monthly, key_min_sent, key_min_rcvd, app_min_sent, app_min_rcvd = unpack(
    redis.call('MGET',
        monthly_quota_key,
        per_key_minute_key,
        per_key_minute_key,  -- Will use HGETM below instead
        per_app_minute_key,
        per_app_minute_key
    )
)

-- Redis doesn't support MGET for hash fields, switch to pipeline
-- Re-read using proper hash field access
local monthly_usage = tonumber(redis.call('GET', monthly_quota_key) or 0)
local key_minute_usage = tonumber(redis.call('HGET', per_key_minute_key, 'total') or 0)
local app_minute_usage = tonumber(redis.call('HGET', per_app_minute_key, 'total') or 0)

-- Step 2: Check monthly quota (application-level, shared across all keys)
if monthly_usage >= app_monthly_limit then
    return 1  -- QUOTA_EXCEEDED
end

-- Step 3: Check rate limit (per-key, per-minute)
if key_minute_usage >= key_rate_limit then
    -- Record rate limit hit atomically
    local hit_set = redis.call('SET', ratelimit_hit_key, '1', 'NX', 'EX', hit_key_ttl)
    if hit_set then
        -- Only increment on first hit to prevent double-counting
        redis.call('HINCRBY', per_key_minute_key, 'ratelimit_hits', 1)
        redis.call('HINCRBY', per_app_minute_key, 'ratelimit_hits', 1)
    end
    return 2  -- RATE_LIMIT_EXCEEDED
end

-- Step 4: Return OK - caller should proceed with increment_usage script
return 0  -- OK
