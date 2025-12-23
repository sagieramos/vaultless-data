-- =============================================================================
-- send_message_v1.lua
-- =============================================================================
-- Atomically sends an instant message with all operations in single round-trip.
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (8 keys, O(1)):
--   KEYS[1] = Monthly quota key (e.g., "quota:app:{app_id}:monthly")
--   KEYS[2] = Per-key per-minute rate limit key (e.g., "metric:key:{sk_id}:minute:{min_key}")
--   KEYS[3] = Session sent metric key (e.g., "metric:session:{session_id}:sent")
--   KEYS[4] = Session bytes metric key (e.g., "metric:session:{session_id}:bytes_sent")
--   KEYS[5] = Session proved metric key (e.g., "metric:session:{session_id}:proved")
--   KEYS[6] = Message idempotency key (e.g., "counted:msg:{msg_id}")
--   KEYS[7] = Message cache key (e.g., "instant_message:message:{msg_id}")
--   KEYS[8] = Recipient inbox queue key (e.g., "instant_message:inbox:{recipient_client_id}")
--
-- ARGUMENTS (12 args):
--   ARGV[1]  = Application monthly quota limit (messages)
--   ARGV[2]  = Per-key rate limit per minute
--   ARGV[3]  = Session metric TTL (seconds, ~7 days)
--   ARGV[4]  = Idempotency key TTL (seconds, 1 hour)
--   ARGV[5]  = Message cache TTL (seconds, 10 minutes)
--   ARGV[6]  = Inbox max length (keep last N messages)
--   ARGV[7]  = Message JSON (serialized Message struct)
--   ARGV[8]  = Content size in bytes
--   ARGV[9]  = Session ID (for metrics key construction)
--   ARGV[10] = Recipient client ID (for inbox key construction)
--   ARGV[11] = Proof verified flag (1 = verified, 0 = not verified)
--   ARGV[12] = Rate limit hit idempotency key suffix (for recording hits)
--
-- RETURN TABLE (always returns table):
--   {status, counted, remaining_quota, error_details}
--
-- STATUS CODES:
--   "OK"              = Success, message sent and counted
--   "QUOTA_EXCEEDED"  = Monthly quota exceeded, message not sent
--   "RATE_LIMIT_EXCEEDED" = Per-minute rate limit exceeded
--   "DUPLICATE"       = Message already sent (idempotency), not sent again
--   "ERROR"           = Redis error (check error_details)
--
-- ATOMICITY GUARANTEES:
--   1. Idempotency check first via SET NX EX
--   2. If duplicate: return immediately, no state changes
--   3. Check monthly quota and rate limit
--   4. If rate limited: record hit and return
--   5. If allowed: atomically increment all metrics
--   6. Store message and enqueue inbox in same script
--   7. All-or-nothing: script fails entirely on any error
--
-- IDEMPOTENCY:
--   Uses KEYS[6] with SET NX EX - only first sender succeeds
--   Safe to retry on network errors without double-counting
-- =============================================================================

local monthly_quota_key = KEYS[1]
local rate_limit_key = KEYS[2]
local session_sent_key = KEYS[3]
local session_bytes_key = KEYS[4]
local session_proved_key = KEYS[5]
local idempotency_key = KEYS[6]
local message_cache_key = KEYS[7]
local inbox_queue_key = KEYS[8]

local app_monthly_limit = tonumber(ARGV[1])
local key_rate_limit = tonumber(ARGV[2])
local session_ttl = tonumber(ARGV[3])
local idempotency_ttl = tonumber(ARGV[4])
local cache_ttl = tonumber(ARGV[5])
local inbox_max_len = tonumber(ARGV[6])
local message_json = ARGV[7]
local size_bytes = tonumber(ARGV[8])
local proof_verified = tonumber(ARGV[11])
local ratelimit_hit_suffix = ARGV[12]

-- ============================================================================
-- STEP 1: Idempotency check (must be first)
-- ============================================================================
local was_set = redis.call('SET', idempotency_key, '1', 'NX', 'EX', idempotency_ttl)
if not was_set then
    -- Message already sent, return DUPLICATE
    return {"DUPLICATE", 0, 0, nil}
end

-- ============================================================================
-- STEP 2: Check monthly quota
-- ============================================================================
local current_monthly = tonumber(redis.call('GET', monthly_quota_key) or 0)
if current_monthly >= app_monthly_limit then
    -- Quota exceeded - rollback idempotency key and return
    redis.call('DEL', idempotency_key)
    return {"QUOTA_EXCEEDED", 0, 0, "Monthly quota limit reached"}
end

-- ============================================================================
-- STEP 3: Check per-key rate limit (per-minute)
-- ============================================================================
local current_rate = tonumber(redis.call('HGET', rate_limit_key, 'total') or 0)
if current_rate >= key_rate_limit then
    -- Rate limit exceeded - record hit and rollback
    local ratelimit_hit_key = "counted:ratelimit:" .. ratelimit_hit_suffix
    redis.call('SET', ratelimit_hit_key, '1', 'NX', 'EX', 3600)  -- 1 hour TTL
    redis.call('HINCRBY', rate_limit_key, 'ratelimit_hits', 1)
    redis.call('DEL', idempotency_key)
    return {"RATE_LIMIT_EXCEEDED", 0, 0, "Per-minute rate limit exceeded"}
end

-- ============================================================================
-- STEP 4: Increment all metrics atomically
-- ============================================================================

-- Application monthly quota
local new_monthly = redis.call('INCR', monthly_quota_key)
if new_monthly == 1 then
    redis.call('EXPIRE', monthly_quota_key, 31 * 24 * 60 * 60)  -- ~31 days
end

-- Per-key rate limit counter (increment AFTER check)
redis.call('HINCRBY', rate_limit_key, 'total', 1)
redis.call('HINCRBY', rate_limit_key, 'messages', 1)
redis.call('HINCRBY', rate_limit_key, 'bytes', size_bytes)
-- Set TTL only on first creation
if redis.call('EXISTS', rate_limit_key) == 0 then
    redis.call('EXPIRE', rate_limit_key, 90)  -- 90 seconds
end

-- Session sent count
redis.call('INCR', session_sent_key)
redis.call('EXPIRE', session_sent_key, session_ttl)

-- Session bytes sent
redis.call('INCRBY', session_bytes_key, size_bytes)
redis.call('EXPIRE', session_bytes_key, session_ttl)

-- Session proved count (only if proof was verified)
if proof_verified == 1 then
    redis.call('INCR', session_proved_key)
    redis.call('EXPIRE', session_proved_key, session_ttl)
end

-- ============================================================================
-- STEP 5: Store message in cache
-- ============================================================================
redis.call('SET', message_cache_key, message_json, 'EX', cache_ttl)

-- ============================================================================
-- STEP 6: Enqueue to recipient inbox (bounded queue)
-- ============================================================================
redis.call('RPUSH', inbox_queue_key, message_json)
redis.call('LTRIM', inbox_queue_key, -inbox_max_len, -1)

-- ============================================================================
-- STEP 7: Return success
-- ============================================================================
local remaining = app_monthly_limit - new_monthly
return {"OK", 1, remaining, nil}
