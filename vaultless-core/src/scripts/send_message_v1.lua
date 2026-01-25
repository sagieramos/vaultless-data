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
-- KEY STRUCTURE (9 keys):
--   KEYS[1] = Application monthly quota key (e.g., "quota:app:{app_id}:monthly")
--   KEYS[2] = Application per-minute rate limit key (e.g., "metric:app:{app_id}:minute:{min_key}")
--   KEYS[3] = Client per-minute rate limit key (e.g., "metric:client:{client_id}:minute:{min_key}")
--   KEYS[4] = Message idempotency key (e.g., "counted:msg:{msg_id}")
--   KEYS[5] = Message stream key (e.g., "stream:instant_message:pending")
--   KEYS[6] = Client monthly quota key (e.g., "quota:client:{client_id}:monthly")
--   KEYS[7] = Client metric key (hash, e.g., "metric:client:hour:{app_id}:{client_id}:{window}")
--   KEYS[8] = Client active keys set (for flusher tracking, e.g., "metric:client:active_keys")
--   KEYS[9] = App active clients set (e.g., "metric:app:{app_id}:active_clients") - for O(1) SCARD
--
-- ARGUMENTS (16 args):
--   ARGV[1]  = Application monthly quota limit (messages)
--   ARGV[2]  = Application per-minute rate limit
--   ARGV[3]  = Client monthly quota limit (messages)
--   ARGV[4]  = Client per-minute rate limit
--   ARGV[5]  = Idempotency key TTL (seconds, 1 hour)
--   ARGV[6]  = Message stream max length (entries, 100000)
--   ARGV[7]  = Message ID (UUID)
--   ARGV[8]  = Message JSON (serialized Message struct)
--   ARGV[9]  = Content size in bytes
--   ARGV[10] = Recipient client ID (for warm path processing)
--   ARGV[11] = Proof verified flag (1 = verified, 0 = not verified)
--   ARGV[12] = Persist to DB flag (1 = persist to Postgres, 0 = Redis only)
--   ARGV[13] = Client metric TTL (seconds, ~7 days)
--   ARGV[14] = Sender client ID
--   ARGV[15] = Application ID
--   ARGV[16] = Active clients set TTL (seconds, 24 hours)
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
--   - Atomic metrics increment (application + client)
--   - Emit to Redis Stream for warm path
--
--   WARM PATH (separate workers): async, scalable, consumer groups
--   - Store message in cache with TTL
--   - Enqueue to recipient inbox (bounded)
--   - Persist to Postgres if flag is set
--   - Acknowledgment via stream consumer groups
--
-- IDEMPOTENCY:
--   Uses KEYS[4] with SET NX EX - only first sender succeeds
--   Safe to retry on network errors without double-counting
-- =============================================================================

local app_quota_key = KEYS[1]
local app_rate_limit_key = KEYS[2]
local client_rate_limit_key = KEYS[3]
local idempotency_key = KEYS[4]
local message_stream_key = KEYS[5]

local client_quota_key = KEYS[6]
local client_metric_key = KEYS[7]
local client_active_keys_set = KEYS[8]
local app_active_clients_set = KEYS[9]

local app_monthly_limit = tonumber(ARGV[1])
local app_rate_limit = tonumber(ARGV[2])
local client_monthly_limit = tonumber(ARGV[3])
local sender_client_id = ARGV[14]
local application_id = ARGV[15]
local active_clients_ttl = tonumber(ARGV[16])
local client_rate_limit = tonumber(ARGV[4])
local idempotency_ttl = tonumber(ARGV[5])
local message_id = ARGV[7]
local message_json = ARGV[8]
local size_bytes = tonumber(ARGV[9])
local recipient_client_id = ARGV[10]
local proof_verified = tonumber(ARGV[11])
local persist_to_db = tonumber(ARGV[12])
local client_metric_ttl = tonumber(ARGV[13])

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

-- Track sender in per-app active clients set for O(1) count via SCARD
-- Set contains only client_ids (not full metric keys) for space efficiency
redis.call('SADD', app_active_clients_set, sender_client_id)
redis.call('EXPIRE', app_active_clients_set, active_clients_ttl)

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
           'application_id', application_id,
           'persist_to_db', tostring(persist_to_db),
           'cache_ttl', '600')  -- 10 minutes for message cache

-- ============================================================================
-- STEP 9: Return success
-- ============================================================================
local remaining = app_monthly_limit - new_app_monthly
return {"OK", 1, remaining, nil}
