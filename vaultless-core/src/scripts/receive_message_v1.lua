-- =============================================================================
-- receive_message_v1.lua
-- =============================================================================
-- Atomically processes message delivery/receipt with metrics tracking.
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (5 keys, O(1)):
--   KEYS[1] = Delivery idempotency key (e.g., "counted:delivered:{msg_id}")
--   KEYS[2] = Application hourly metrics (e.g., "metric:app:{app_id}:hourly:{hour_key}")
--   KEYS[3] = Message cache key (e.g., "instant_message:message:{msg_id}")
--   KEYS[4] = Message delivered status key (e.g., "delivered:{msg_id}")
--   KEYS[5] = Per-app per-minute rate limit key (e.g., "metric:app:{app_id}:minute:{min_key}")
--
-- ARGUMENTS (10 args):
--   ARGV[1]  = Application ID (UUID string)
--   ARGV[2]  = Content size in bytes
--   ARGV[3]  = Hourly metric TTL (seconds, ~2 hours)
--   ARGV[4]  = Delivered idempotency key TTL (seconds, 24 hours)
--   ARGV[5]  = Message cache TTL (seconds, 10 minutes)
--   ARGV[6]  = Delivered status TTL (seconds, 24 hours)
--   ARGV[7]  = Per-app per-minute rate limit
--   ARGV[8]  = Per-app rate limit hit TTL (seconds)
--   ARGV[9]  = Rate limit hit idempotency suffix (for recording hits)
--   ARGV[10] = Current hour key (for hourly metrics)
--
-- RETURN TABLE:
--   {status, was_new_delivery, remaining_quota, error_details}
--
-- STATUS CODES:
--   "OK_NEW"           = First delivery, counted successfully
--   "OK_ALREADY"       = Already delivered previously, not counted again
--   "RATE_LIMIT_EXCEEDED" = Per-minute rate limit exceeded
--   "ERROR"            = Redis error (check error_details)
--
-- ATOMICITY GUARANTEES:
--   1. Idempotency check first via SET NX EX
--   2. If already delivered: return immediately, no metrics
--   3. Check per-app rate limit (for receives)
--   4. If rate limited: record hit and return
--   5. If allowed: atomically increment all metrics
--   6. Mark as delivered in single operation
--   7. All-or-nothing: script fails entirely on any error
-- =============================================================================

local delivered_idempotency_key = KEYS[1]
local app_hourly_key = KEYS[2]
local message_cache_key = KEYS[3]
local delivered_status_key = KEYS[4]
local app_rate_limit_key = KEYS[5]

local app_id = ARGV[1]
local size_bytes = tonumber(ARGV[2])
local hourly_ttl = tonumber(ARGV[3])
local delivered_ttl = tonumber(ARGV[4])
local cache_ttl = tonumber(ARGV[5])
local status_ttl = tonumber(ARGV[6])
local app_rate_limit = tonumber(ARGV[7])
local rate_limit_hit_ttl = tonumber(ARGV[8])
local rate_limit_hit_suffix = ARGV[9]
local hour_key = ARGV[10]

-- ============================================================================
-- STEP 1: Delivery idempotency check (must be first)
-- ============================================================================
local was_set = redis.call('SET', delivered_idempotency_key, '1', 'NX', 'EX', delivered_ttl)
if not was_set then
    -- Already delivered, return without counting
    return {"OK_ALREADY", 0, 0, nil}
end

-- ============================================================================
-- STEP 2: Check per-app rate limit (for receive operations)
-- ============================================================================
local current_rate = tonumber(redis.call('HGET', app_rate_limit_key, 'total') or 0)
if current_rate >= app_rate_limit then
    -- Rate limit exceeded - record hit and rollback
    local ratelimit_hit_key = "counted:ratelimit:recv:" .. rate_limit_hit_suffix
    redis.call('SET', ratelimit_hit_key, '1', 'NX', 'EX', rate_limit_hit_ttl)
    redis.call('HINCRBY', app_rate_limit_key, 'ratelimit_hits', 1)
    redis.call('DEL', delivered_idempotency_key)
    return {"RATE_LIMIT_EXCEEDED", 0, 0, "Per-minute receive rate limit exceeded"}
end

-- ============================================================================
-- STEP 3: Increment application-level metrics
-- ============================================================================

-- Hourly metrics (hash)
local current_hourly = redis.call('HINCRBY', app_hourly_key, 'messages_received', 1)
local current_bytes = redis.call('HINCRBY', app_hourly_key, 'total_bytes_received', size_bytes)
local current_total = redis.call('HINCRBY', app_hourly_key, 'total', 1)

-- Set TTL only on first creation
if redis.call('EXISTS', app_hourly_key) == 0 then
    redis.call('EXPIRE', app_hourly_key, hourly_ttl)
end

-- Per-app rate limit counter (increment AFTER check)
redis.call('HINCRBY', app_rate_limit_key, 'total', 1)
redis.call('HINCRBY', app_rate_limit_key, 'messages_received', 1)
redis.call('HINCRBY', app_rate_limit_key, 'bytes_received', size_bytes)
-- Set TTL only on first creation
if redis.call('EXISTS', app_rate_limit_key) == 0 then
    redis.call('EXPIRE', app_rate_limit_key, 90)  -- 90 seconds
end

-- ============================================================================
-- STEP 4: Mark as delivered (update message cache if exists)
-- ============================================================================
local message_data = redis.call('GET', message_cache_key)
if message_data then
    -- Parse and update message as delivered
    -- The message is stored as JSON, we need to update delivered status
    -- We'll append delivery timestamp to mark it
    local delivered_at = redis.call('TIME')[1]  -- Unix timestamp
    redis.call('HSET', message_cache_key, 'is_delivered', 'true', 'delivered_at', delivered_at)
    redis.call('EXPIRE', message_cache_key, cache_ttl)
end

-- Store delivered status separately for quick lookup
local delivered_at = redis.call('TIME')[1]  -- Unix timestamp
redis.call('SET', delivered_status_key, delivered_at, 'EX', status_ttl)

-- ============================================================================
-- STEP 5: Return success
-- ============================================================================
local remaining_rate = app_rate_limit - (current_rate + 1)
return {"OK_NEW", 1, remaining_rate, nil}
