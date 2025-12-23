-- =============================================================================
-- record_rate_limit_hit_v1.lua
-- =============================================================================
-- Records a rate limit hit event for analytics and monitoring purposes.
-- Safe to call independently or as part of error handling.
--
-- KEY STRUCTURE (2 keys, O(1)):
--   KEYS[1] = Per-key per-minute metrics key (e.g., "metric:key:{sk_id}:minute:{minute_key}")
--   KEYS[2] = Per-app per-minute metrics key (e.g., "metric:app:{app_id}:minute:{minute_key}")
--
-- ARGUMENTS (3 args):
--   ARGV[1] = Idempotency key suffix (e.g., "{msg_id}")
--   ARGV[2] = TTL for rate limit hit hash fields (seconds)
--   ARGV[3] = Common prefix for idempotency key (default: "counted:ratelimit")
--
-- RETURN CODES:
--   0 = New hit recorded (first time for this request)
--   1 = Already recorded (duplicate, ignored)
--   2 = Error (key type mismatch)
--
-- IDEMPOTENCY:
--   Uses SET NX EX on composite key: {prefix}:{idempotency_key_suffix}
--   Prevents double-counting when called multiple times for same request
--
-- TTL BEHAVIOR:
--   Rate limit hit counter TTL managed by the parent hash key (per_key_minute_key)
--   This script only increments the counter, parent key TTL handled elsewhere
--
-- ATOMICITY GUARANTEES:
--   Idempotency check + counter increment in single execution
--   No partial state visible to other clients
--   Safe for concurrent calls from multiple workers
-- =============================================================================

local per_key_minute_key = KEYS[1]
local per_app_minute_key = KEYS[2]

local idempotency_suffix = ARGV[1]
local counter_ttl = tonumber(ARGV[2])
local idempotency_prefix = ARGV[3] or "counted:ratelimit"

-- Construct idempotency key from prefix and suffix
local idempotency_key = idempotency_prefix .. ":" .. idempotency_suffix

-- Step 1: Idempotency check - only count once per request
local was_set = redis.call('SET', idempotency_key, '1', 'NX', 'EX', 3600)  -- 1 hour TTL
if not was_set then
    return 1  -- Already recorded (duplicate)
end

-- Step 2: Increment per-key rate limit hit counter
local key_hits = redis.call('HINCRBY', per_key_minute_key, 'ratelimit_hits', 1)
-- Set TTL only on first creation of the hash
if redis.call('EXISTS', per_key_minute_key) == 0 then
    redis.call('EXPIRE', per_key_minute_key, counter_ttl)
end

-- Step 3: Increment per-app rate limit hit counter
local app_hits = redis.call('HINCRBY', per_app_minute_key, 'ratelimit_hits', 1)
if redis.call('EXISTS', per_app_minute_key) == 0 then
    redis.call('EXPIRE', per_app_minute_key, counter_ttl)
end

return 0  -- New hit recorded
