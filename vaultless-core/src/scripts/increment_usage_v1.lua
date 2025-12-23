-- =============================================================================
-- increment_usage_v1.lua
-- =============================================================================
-- Atomically increments usage counters after successful request processing.
-- Updates application-level and client-level counters in single atomic operation.
--
-- KEY STRUCTURE (5 keys, O(1)):
--   KEYS[1] = Monthly quota key for application (e.g., "quota:app:{app_id}:monthly")
--   KEYS[2] = Monthly bytes quota key for application (e.g., "quota:app:{app_id}:bytes:monthly")
--   KEYS[3] = Per-key per-minute metrics key (e.g., "metric:key:{sk_id}:minute:{minute_key}")
--   KEYS[4] = Per-app per-minute metrics key (e.g., "metric:app:{app_id}:minute:{minute_key}")
--   KEYS[5] = Idempotency key for this message (e.g., "counted:msg:{msg_id}")
--
-- ARGUMENTS (7 args):
--   ARGV[1] = Message size in bytes
--   ARGV[2] = TTL for monthly quota keys (seconds, ~31 days)
--   ARGV[3] = TTL for monthly bytes keys (seconds, ~31 days)
--   ARGV[4] = TTL for per-key minute metrics (seconds, ~90 seconds)
--   ARGV[5] = TTL for per-app minute metrics (seconds, ~90 seconds)
--   ARGV[6] = Idempotency key TTL (seconds, 1 hour)
--   ARGV[7] = Client ID (for client-level tracking)
--
-- RETURN VALUES:
--   {new_monthly_count, new_monthly_bytes, new_key_minute_total, new_app_minute_total, was_counted}
--   was_counted = 0 if this was first time, 1 if duplicate (idempotency)
--
-- IDEMPOTENCY:
--   Uses SET NX EX on KEYS[5] to ensure only first call increments counters
--   Safe to retry on network errors without double-counting
--
-- ATOMICITY GUARANTEES:
--   All 5 counters increment in single Lua execution
--   TTL only set on first creation of each key
--   No partial completion possible
-- =============================================================================

local monthly_quota_key = KEYS[1]
local monthly_bytes_key = KEYS[2]
local per_key_minute_key = KEYS[3]
local per_app_minute_key = KEYS[4]
local idempotency_key = KEYS[5]

local size_bytes = tonumber(ARGV[1])
local monthly_quota_ttl = tonumber(ARGV[2])
local monthly_bytes_ttl = tonumber(ARGV[3])
local per_key_ttl = tonumber(ARGV[4])
local per_app_ttl = tonumber(ARGV[5])
local idempotency_ttl = tonumber(ARGV[6])
local client_id = ARGV[7]

-- Step 1: Idempotency check - only count once per message
local already_counted = redis.call('SET', idempotency_key, '1', 'NX', 'EX', idempotency_ttl)
if not already_counted then
    -- Duplicate request, return current values without incrementing
    local current_monthly = tonumber(redis.call('GET', monthly_quota_key) or 0)
    local current_monthly_bytes = tonumber(redis.call('GET', monthly_bytes_key) or 0)
    local current_key_minute = tonumber(redis.call('HGET', per_key_minute_key, 'total') or 0)
    local current_app_minute = tonumber(redis.call('HGET', per_app_minute_key, 'total') or 0)
    return {current_monthly, current_monthly_bytes, current_key_minute, current_app_minute, 1}
end

-- Step 2: Increment application monthly message count
local new_monthly_count = redis.call('INCR', monthly_quota_key)
if new_monthly_count == 1 then
    redis.call('EXPIRE', monthly_quota_key, monthly_quota_ttl)
end

-- Step 3: Increment application monthly bytes count
local new_monthly_bytes = redis.call('INCRBY', monthly_bytes_key, size_bytes)
if new_monthly_bytes == size_bytes then
    -- First time creating this key (value equals increment)
    redis.call('EXPIRE', monthly_bytes_key, monthly_bytes_ttl)
end

-- Step 4: Increment per-key per-minute metrics
-- Using HINCRBY for multiple fields in single call
redis.call('HINCRBY', per_key_minute_key, 'messages', 1)
redis.call('HINCRBY', per_key_minute_key, 'bytes', size_bytes)
redis.call('HINCRBY', per_key_minute_key, 'total', 1)

-- Get total for return value
local new_key_total = redis.call('HGET', per_key_minute_key, 'total')
-- Set TTL only on first creation
if redis.call('EXISTS', per_key_minute_key) == 0 then
    redis.call('EXPIRE', per_key_minute_key, per_key_ttl)
end

-- Step 5: Increment per-app per-minute metrics
redis.call('HINCRBY', per_app_minute_key, 'messages', 1)
redis.call('HINCRBY', per_app_minute_key, 'bytes', size_bytes)
redis.call('HINCRBY', per_app_minute_key, 'total', 1)

-- Get total for return value
local new_app_total = redis.call('HGET', per_app_minute_key, 'total')
-- Set TTL only on first creation
if redis.call('EXISTS', per_app_minute_key) == 0 then
    redis.call('EXPIRE', per_app_minute_key, per_app_ttl)
end

return {
    tonumber(new_monthly_count),
    tonumber(new_monthly_bytes),
    tonumber(new_key_total),
    tonumber(new_app_total),
    0  -- was_counted = 0 (first time)
}
