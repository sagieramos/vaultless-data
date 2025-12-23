-- =============================================================================
-- send_message_v1.lua (Hot Path)
-- =============================================================================
-- Atomic hot path for instant message sending: single round-trip handles
-- idempotency, quota check, rate limit check, metrics increment, and stream emit.
-- Message storage and inbox enqueue are handled asynchronously by warm path workers
-- consuming from the message stream.
--
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (7 keys):
--   KEYS[1] = Monthly quota key (e.g., "quota:app:{app_id}:monthly")
--   KEYS[2] = Per-key per-minute rate limit key (e.g., "metric:key:{sk_id}:minute:{min_key}")
--   KEYS[3] = Session sent metric key (e.g., "metric:session:{session_id}:sent")
--   KEYS[4] = Session bytes metric key (e.g., "metric:session:{session_id}:bytes_sent")
--   KEYS[5] = Session proved metric key (e.g., "metric:session:{session_id}:proved")
--   KEYS[6] = Message idempotency key (e.g., "counted:msg:{msg_id}")
--   KEYS[7] = Message stream key (e.g., "stream:instant_message:pending")
--
-- ARGUMENTS (11 args):
--   ARGV[1]  = Application monthly quota limit (messages)
--   ARGV[2]  = Per-key rate limit per minute
--   ARGV[3]  = Session metric TTL (seconds, ~7 days)
--   ARGV[4]  = Idempotency key TTL (seconds, 1 hour)
--   ARGV[5]  = Message stream max length (entries, 100000)
--   ARGV[6]  = Message ID (UUID)
--   ARGV[7]  = Message JSON (serialized Message struct)
--   ARGV[8]  = Content size in bytes
--   ARGV[9]  = Session ID (for metrics key construction)
--   ARGV[10] = Recipient client ID (for warm path processing)
--   ARGV[11] = Proof verified flag (1 = verified, 0 = not verified)
--
-- RETURN TABLE (always returns table):
--   {status, counted, remaining_quota, error_details}
--
-- STATUS CODES:
--   "OK"                  = Success, message queued for async processing
--   "QUOTA_EXCEEDED"      = Monthly quota exceeded, message not sent
--   "RATE_LIMIT_EXCEEDED" = Per-minute rate limit exceeded
--   "DUPLICATE"           = Message already sent (idempotency), not sent again
--   "ERROR"               = Redis error (check error_details)
--
-- ARCHITECTURE:
--   HOT PATH (this script):    ~1ms latency, atomic, single round-trip
--   - Idempotency check via SET NX EX
--   - Monthly quota validation
--   - Per-key rate limit check
--   - Atomic metrics increment
--   - Emit to Redis Stream for warm path
--
--   WARM PATH (separate workers): async, scalable, consumer groups
--   - Store message in cache with TTL
--   - Enqueue to recipient inbox (bounded)
--   - Acknowledgment via stream consumer groups
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
local message_stream_key = KEYS[7]

local app_monthly_limit = tonumber(ARGV[1])
local key_rate_limit = tonumber(ARGV[2])
local session_ttl = tonumber(ARGV[3])
local idempotency_ttl = tonumber(ARGV[4])
local stream_max_len = tonumber(ARGV[5])
local message_id = ARGV[6]
local message_json = ARGV[7]
local size_bytes = tonumber(ARGV[8])
local proof_verified = tonumber(ARGV[11])

-- ============================================================================
-- STEP 1: Idempotency check (must be first, using unique value for safe rollback)
-- ============================================================================
local was_set = redis.call('SET', idempotency_key, message_id, 'NX', 'EX', idempotency_ttl)
if not was_set then
    -- Message already sent, return DUPLICATE
    return {"DUPLICATE", 0, 0, nil}
end

-- ============================================================================
-- STEP 2: Check monthly quota (INCR first, then validate atomically)
-- ============================================================================
local new_monthly = redis.call('INCR', monthly_quota_key)
if new_monthly == 1 then
    redis.call('EXPIRE', monthly_quota_key, 31 * 24 * 60 * 60)  -- ~31 days
end

-- Rollback if quota exceeded (must hold idempotency lock to prevent race)
if new_monthly > app_monthly_limit then
    redis.call('DECR', monthly_quota_key)
    -- Only delete if we still own the lock (compare-and-delete pattern via GETDEL)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return {"QUOTA_EXCEEDED", 0, 0, "Monthly quota limit reached"}
end

-- ============================================================================
-- STEP 3: Check per-key rate limit (INCR first, then validate atomically)
-- ============================================================================
-- Increment rate limit counter first (atomic), then validate
local new_rate = redis.call('HINCRBY', rate_limit_key, 'total', 1)
if new_rate == 1 then
    -- First entry - set TTL
    redis.call('EXPIRE', rate_limit_key, 90)  -- 90 seconds per-minute window
end

if new_rate > key_rate_limit then
    -- Rate limit exceeded - rollback and return
    redis.call('HINCRBY', rate_limit_key, 'total', -1)
    redis.call('DECR', monthly_quota_key)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return {"RATE_LIMIT_EXCEEDED", 0, 0, "Per-minute rate limit exceeded"}
end

-- ============================================================================
-- STEP 4: Increment remaining metrics atomically
-- ============================================================================

-- Per-key rate limit: increment other fields (messages, bytes)
redis.call('HINCRBY', rate_limit_key, 'messages', 1)
redis.call('HINCRBY', rate_limit_key, 'bytes', size_bytes)

-- Session sent count (set TTL only on first creation)
local new_sent = redis.call('INCR', session_sent_key)
if new_sent == 1 then
    redis.call('EXPIRE', session_sent_key, session_ttl)
end

-- Session bytes sent (set TTL only on first creation)
local new_bytes = redis.call('INCRBY', session_bytes_key, size_bytes)
if new_bytes == size_bytes then
    redis.call('EXPIRE', session_bytes_key, session_ttl)
end

-- Session proved count (only if proof was verified, set TTL only on first creation)
if proof_verified == 1 then
    local new_proved = redis.call('INCR', session_proved_key)
    if new_proved == 1 then
        redis.call('EXPIRE', session_proved_key, session_ttl)
    end
end

-- ============================================================================
-- STEP 5: Emit to message stream (warm path processes storage + inbox)
-- ============================================================================
redis.call('XADD', message_stream_key, 'MAXLEN', '~', stream_max_len,
           '*',
           'message_id', message_id,
           'message_json', message_json,
           'size_bytes', tostring(size_bytes),
           'recipient_client_id', ARGV[10],
           'cache_ttl', '600')  -- 10 minutes for message cache

-- ============================================================================
-- STEP 6: Return success
-- ============================================================================
local remaining = app_monthly_limit - new_monthly
return {"OK", 1, remaining, nil}
