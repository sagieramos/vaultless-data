-- =============================================================================
-- send_message_v1.lua (Hot Path)
-- =============================================================================
-- Atomic hot path for instant message sending: single round-trip handles
-- idempotency, quota check (app + client), rate limit check (app + client),
-- metrics increment, and stream emit. Message storage and inbox enqueue are
-- handled asynchronously by warm path workers consuming from the message stream.
--
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (12 keys):
--   KEYS[1] = Application monthly quota key (e.g., "quota:app:{app_id}:monthly")
--   KEYS[2] = Application per-minute rate limit key (e.g., "metric:app:{app_id}:minute:{min_key}")
--   KEYS[3] = Client per-minute rate limit key (e.g., "metric:client:{client_id}:minute:{min_key}")
--   KEYS[4] = Session sent metric key (e.g., "metric:session:{session_id}:sent")
--   KEYS[5] = Session bytes metric key (e.g., "metric:session:{session_id}:bytes_sent")
--   KEYS[6] = Session proofs_verified metric key (e.g., "metric:session:{session_id}:proofs_verified")
--   KEYS[7] = Message idempotency key (e.g., "counted:msg:{msg_id}")
--   KEYS[8] = Message stream key (e.g., "stream:instant_message:pending")
--   KEYS[9] = Client monthly quota key (e.g., "quota:client:{client_id}:monthly")
--   KEYS[10] = Client metric key (hash, e.g., "metric:client:hour:{app_id}:{client_id}:{window}")
--   KEYS[11] = Client active keys set (for flusher tracking, e.g., "metric:client:active_keys")
--
-- ARGUMENTS (16 args):
--   ARGV[1]  = Application monthly quota limit (messages)
--   ARGV[2]  = Application per-minute rate limit
--   ARGV[3]  = Client monthly quota limit (messages)
--   ARGV[4]  = Client per-minute rate limit
--   ARGV[5]  = Session metric TTL (seconds, ~7 days)
--   ARGV[6]  = Idempotency key TTL (seconds, 1 hour)
--   ARGV[7]  = Message stream max length (entries, 100000)
--   ARGV[8]  = Message ID (UUID)
--   ARGV[9]  = Message JSON (serialized Message struct)
--   ARGV[10] = Content size in bytes
--   ARGV[11] = Session ID (for metrics key construction)
--   ARGV[12] = Recipient client ID (for warm path processing)
--   ARGV[13] = Proof verified flag (1 = verified, 0 = not verified)
--   ARGV[14] = Persist to DB flag (1 = persist to Postgres, 0 = Redis only)
--   ARGV[15] = Client metric TTL (seconds, ~7 days)
--   ARGV[16] = Sender client ID
--
-- RETURN TABLE (always returns table):
--   {status, counted, remaining_quota, error_details}
--
-- STATUS CODES:
--   "OK"                  = Success, message queued for async processing
--   "QUOTA_EXCEEDED"      = Monthly quota exceeded (app or client), message not sent
--   "RATE_LIMIT_EXCEEDED" = Per-minute rate limit exceeded (app or client)
--   "DUPLICATE"           = Message already sent (idempotency), not sent again
--   "ERROR"               = Redis error (check error_details)
--
-- ARCHITECTURE:
--   HOT PATH (this script):    ~1ms latency, atomic, single round-trip
--   - Idempotency check via SET NX EX
--   - Application monthly quota validation
--   - Application per-minute rate limit check
--   - Client monthly quota validation
--   - Client per-minute rate limit check
--   - Atomic metrics increment (application + session + client)
--   - Emit to Redis Stream for warm path
--
--   WARM PATH (separate workers): async, scalable, consumer groups
--   - Store message in cache with TTL
--   - Enqueue to recipient inbox (bounded)
--   - Persist to Postgres if flag is set
--   - Acknowledgment via stream consumer groups
--
-- IDEMPOTENCY:
--   Uses KEYS[7] with SET NX EX - only first sender succeeds
--   Safe to retry on network errors without double-counting
-- =============================================================================

local app_quota_key = KEYS[1]
local app_rate_limit_key = KEYS[2]
local client_rate_limit_key = KEYS[3]
local session_sent_key = KEYS[4]
local session_bytes_key = KEYS[5]
local session_proofs_verified_key = KEYS[6]
local idempotency_key = KEYS[7]
local message_stream_key = KEYS[8]

local client_quota_key = KEYS[9]
local client_metric_key = KEYS[10]
local client_active_keys_set = KEYS[11]

local app_monthly_limit = tonumber(ARGV[1])
local app_rate_limit = tonumber(ARGV[2])
local client_monthly_limit = tonumber(ARGV[3])
local client_rate_limit = tonumber(ARGV[4])
local session_ttl = tonumber(ARGV[5])
local idempotency_ttl = tonumber(ARGV[6])
local message_id = ARGV[8]
local message_json = ARGV[9]
local size_bytes = tonumber(ARGV[10])
local recipient_client_id = ARGV[12]
local proof_verified = tonumber(ARGV[13])
local persist_to_db = tonumber(ARGV[14])
local client_metric_ttl = tonumber(ARGV[15])

-- ============================================================================
-- STEP 1: Idempotency check (must be first, using unique value for safe rollback)
-- ============================================================================
local was_set = redis.call('SET', idempotency_key, message_id, 'NX', 'EX', idempotency_ttl)
if not was_set then
    -- Message already sent, return DUPLICATE
    return {"DUPLICATE", 0, 0, nil}
end

-- ============================================================================
-- STEP 2: Check application monthly quota (INCR first, then validate atomically)
-- ============================================================================
local new_app_monthly = redis.call('INCR', app_quota_key)
if new_app_monthly == 1 then
    redis.call('EXPIRE', app_quota_key, 31 * 24 * 60 * 60)  -- ~31 days
end

-- Rollback if app quota exceeded
if new_app_monthly > app_monthly_limit then
    redis.call('DECR', app_quota_key)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return {"QUOTA_EXCEEDED", 0, 0, "Application monthly quota limit reached"}
end

-- ============================================================================
-- STEP 3: Check application per-minute rate limit (INCR first, then validate)
-- ============================================================================
local new_app_rate = redis.call('HINCRBY', app_rate_limit_key, 'total', 1)
redis.call('EXPIRE', app_rate_limit_key, 90)  -- 90 seconds per-minute window

if new_app_rate > app_rate_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return {"RATE_LIMIT_EXCEEDED", 0, 0, "Application per-minute rate limit exceeded"}
end

-- ============================================================================
-- STEP 4: Check client monthly quota (INCR first, then validate atomically)
-- ============================================================================
local new_client_monthly = redis.call('INCR', client_quota_key)
if new_client_monthly == 1 then
    redis.call('EXPIRE', client_quota_key, 31 * 24 * 60 * 60)  -- ~31 days
end

-- Rollback if client quota exceeded
if new_client_monthly > client_monthly_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    redis.call('DECR', client_quota_key)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return {"QUOTA_EXCEEDED", 0, 0, "Client monthly quota limit reached"}
end

-- ============================================================================
-- STEP 5: Check client per-minute rate limit (INCR first, then validate)
-- ============================================================================
local new_client_rate = redis.call('HINCRBY', client_rate_limit_key, 'total', 1)
redis.call('EXPIRE', client_rate_limit_key, 90)  -- 90 seconds per-minute window

if new_client_rate > client_rate_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    redis.call('DECR', client_quota_key)
    redis.call('HINCRBY', client_rate_limit_key, 'total', -1)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return {"RATE_LIMIT_EXCEEDED", 0, 0, "Client per-minute rate limit exceeded"}
end

-- ============================================================================
-- STEP 6: Increment remaining metrics atomically
-- ============================================================================

-- Application rate limit: increment other fields (messages, bytes)
redis.call('HINCRBY', app_rate_limit_key, 'messages', 1)
redis.call('HINCRBY', app_rate_limit_key, 'bytes', size_bytes)

-- Session sent count (set TTL only on first creation)
local new_sent = redis.call('INCR', session_sent_key)
if new_sent == 1 then
    redis.call('EXPIRE', session_sent_key, session_ttl)
end

-- Session bytes sent (set TTL only on first creation)
local is_new_bytes_key = redis.call('EXISTS', session_bytes_key) == 0
redis.call('INCRBY', session_bytes_key, size_bytes)
if is_new_bytes_key then
    redis.call('EXPIRE', session_bytes_key, session_ttl)
end

-- Session proofs_verified count (only if proof was verified, set TTL only on first creation)
if proof_verified == 1 then
    local new_proofs_verified = redis.call('INCR', session_proofs_verified_key)
    if new_proofs_verified == 1 then
        redis.call('EXPIRE', session_proofs_verified_key, session_ttl)
    end
end

-- ============================================================================
-- STEP 7: Increment sender client metrics (sender pays for sending)
-- ============================================================================
-- Client metrics stored as Redis hash with fields:
--   messages_sent, proofs_verified, total_bytes_sent, rate_limit_hits
-- The flusher reads these via HGETALL and persists to client_usage_metrics.

local is_new_client_key = redis.call('EXISTS', client_metric_key) == 0
if is_new_client_key then
    redis.call('EXPIRE', client_metric_key, client_metric_ttl)
end
redis.call('SADD', client_active_keys_set, client_metric_key)

-- Increment sender metrics
redis.call('HINCRBY', client_metric_key, 'messages_sent', 1)
redis.call('HINCRBY', client_metric_key, 'total_bytes_sent', size_bytes)
if proof_verified == 1 then
    redis.call('HINCRBY', client_metric_key, 'proofs_verified', 1)
end

-- ============================================================================
-- STEP 8: Emit to message stream (warm path processes storage + inbox)
-- ============================================================================
-- Note: Stream trimming handled by janitor script (trim_message_stream_v1.lua)
redis.call('XADD', message_stream_key, '*',
           'message_id', message_id,
           'message_json', message_json,
           'size_bytes', tostring(size_bytes),
           'recipient_client_id', recipient_client_id,
           'persist_to_db', tostring(persist_to_db),
           'cache_ttl', '600')  -- 10 minutes for message cache

-- ============================================================================
-- STEP 9: Return success
-- ============================================================================
local remaining = app_monthly_limit - new_app_monthly
return {"OK", 1, remaining, nil}
