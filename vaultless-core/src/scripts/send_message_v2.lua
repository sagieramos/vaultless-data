-- =============================================================================
-- send_message_v2.lua (Consolidated Hot Path)
-- =============================================================================
-- Ultra-optimized: single round-trip handles auth resolution, quota validation,
-- rate limiting, and message sending. Returns cache miss status if auth not cached.
--
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (10 keys):
--   KEYS[1]  = Auth cache key (e.g., "auth:pk:pk_xxx" or "auth:sk:hash")
--   KEYS[2]  = Application monthly quota key (e.g., "quota:app:{app_id}:monthly")
--   KEYS[3]  = Application per-minute rate limit key (e.g., "metric:app:{app_id}:minute:{min_key}")
--   KEYS[4]  = Client per-minute rate limit key (e.g., "metric:client:{client_id}:minute:{min_key}")
--   KEYS[5]  = Message idempotency key (e.g., "counted:msg:{msg_id}")
--   KEYS[6]  = Message stream key (e.g., "stream:instant_message:pending")
--   KEYS[7]  = Client monthly quota key (e.g., "quota:client:{client_id}:monthly")
--   KEYS[8]  = Client metric key (hash, e.g., "metric:client:{app_id}:{client_id}:{window}:hour")
--   KEYS[9]  = Client active keys set (for flusher tracking, e.g., "metric:client:active_keys")
--   KEYS[10] = App active clients set (e.g., "metric:app:{app_id}:active_clients") - for O(1) SCARD
--
-- ARGUMENTS (12 args):
--   ARGV[1]  = Idempotency key TTL (seconds, 1 hour)
--   ARGV[2]  = Message stream max length (entries, 100000)
--   ARGV[3]  = Message ID (UUID)
--   ARGV[4]  = Message JSON (serialized Message struct)
--   ARGV[5]  = Content size in bytes
--   ARGV[6]  = Recipient client ID (for warm path processing)
--   ARGV[7]  = Proof verified flag (1 = verified, 0 = not verified)
--   ARGV[8]  = Persist to DB flag (1 = persist to Postgres, 0 = Redis only)
--   ARGV[9]  = Client metric TTL (seconds, ~7 days)
--   ARGV[10] = Sender client ID
--   ARGV[11] = Application ID
--   ARGV[12] = Active clients set TTL (seconds, 24 hours)
--
-- RETURN TABLE (always returns table):
--   {status, counted, remaining_quota, error_details}
--
-- STATUS CODES:
--   "OK"                  = Success, message queued for async processing
--   "AUTH_CACHE_MISS"     = Auth not in cache, caller must populate and retry
--   "QUOTA_EXCEEDED"      = Monthly quota exceeded (app or client)
--   "RATE_LIMIT_EXCEEDED" = Per-minute rate limit exceeded (app or client)
--   "FORBIDDEN"           = Application is deactivated
--   "DUPLICATE"           = Message already sent (idempotency)
--   "ERROR"               = Redis error (check error_details)
-- =============================================================================

local auth_cache_key = KEYS[1]
local app_quota_key = KEYS[2]
local app_rate_limit_key = KEYS[3]
local client_rate_limit_key = KEYS[4]
local idempotency_key = KEYS[5]
local message_stream_key = KEYS[6]
local client_quota_key = KEYS[7]
local client_metric_key = KEYS[8]
local client_active_keys_set = KEYS[9]
local app_active_clients_set = KEYS[10]

local idempotency_ttl = tonumber(ARGV[1])
local message_id = ARGV[3]
local message_json = ARGV[4]
local size_bytes = tonumber(ARGV[5])
local recipient_client_id = ARGV[6]
local sender_client_id = ARGV[10]
local application_id = ARGV[11]
local active_clients_ttl = tonumber(ARGV[12])
local proof_verified = tonumber(ARGV[7])
local persist_to_db = tonumber(ARGV[8])
local client_metric_ttl = tonumber(ARGV[9])

-- ============================================================================
-- STEP 0: Load auth config from Redis cache
-- ============================================================================
local auth_vals = redis.call('HGETALL', auth_cache_key)

if #auth_vals == 0 then
    -- Cache miss - signal caller to populate from DB
    return { "AUTH_CACHE_MISS", 0, 0, "Auth config not in cache" }
end

-- Parse auth hash into table
local auth = {}
for i = 1, #auth_vals, 2 do
    auth[auth_vals[i]] = auth_vals[i + 1]
end

-- Validate auth config has required fields
if not auth.is_active or not auth.quota or not auth.rate_limit then
    return { "ERROR", 0, 0, "Invalid auth cache format" }
end

-- Check if application is active
if auth.is_active ~= "1" then
    return { "FORBIDDEN", 0, 0, "Application is deactivated" }
end

-- Extract quota/rate limit values from auth config
local app_monthly_limit = tonumber(auth.quota)
local app_rate_limit = tonumber(auth.rate_limit)
local client_monthly_limit = tonumber(auth.quota) -- Assuming same for now
local client_rate_limit = tonumber(auth.rate_limit)
local app_bandwidth_quota = tonumber(auth.bandwidth_quota_bytes) or 0
local app_bandwidth_rate_limit = tonumber(auth.bandwidth_rate_limit_bytes) or 0
local client_bandwidth_quota = tonumber(auth.bandwidth_quota_bytes) or 0
local client_bandwidth_rate_limit = tonumber(auth.bandwidth_rate_limit_bytes) or 0

-- ============================================================================
-- STEP 1: Idempotency check (must be first, using unique value for safe rollback)
-- ============================================================================
local was_set = redis.call('SET', idempotency_key, message_id, 'NX', 'EX', idempotency_ttl)
if not was_set then
    -- Message already sent, return DUPLICATE
    return { "DUPLICATE", 0, 0, nil }
end

-- ============================================================================
-- STEP 2: Check application monthly quota (INCR first, then validate atomically)
-- ============================================================================
local new_app_monthly = redis.call('INCR', app_quota_key)
if new_app_monthly == 1 then
    redis.call('EXPIRE', app_quota_key, 31 * 24 * 60 * 60) -- ~31 days
end

-- Rollback if app quota exceeded
if new_app_monthly > app_monthly_limit then
    redis.call('DECR', app_quota_key)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return { "QUOTA_EXCEEDED", 0, 0, "Application monthly quota limit reached" }
end

-- ============================================================================
-- STEP 3: Check application per-minute rate limit (INCR first, then validate)
-- ============================================================================
local new_app_rate = redis.call('HINCRBY', app_rate_limit_key, 'total', 1)
redis.call('EXPIRE', app_rate_limit_key, 90) -- 90 seconds per-minute window

if new_app_rate > app_rate_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return { "RATE_LIMIT_EXCEEDED", 0, 0, "Application per-minute rate limit exceeded" }
end

-- ============================================================================
-- STEP 3.5: Check application per-minute bandwidth rate limit
-- ============================================================================
local new_app_bytes_rate = redis.call('HINCRBY', app_rate_limit_key, 'bytes_total', size_bytes)

if new_app_bytes_rate > app_bandwidth_rate_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    redis.call('HINCRBY', app_rate_limit_key, 'bytes_total', -size_bytes)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return { "BANDWIDTH_RATE_LIMIT_EXCEEDED", 0, 0, "Application per-minute bandwidth rate limit exceeded" }
end

-- ============================================================================
-- STEP 4: Check client monthly quota (INCR first, then validate atomically)
-- ============================================================================
local new_client_monthly = redis.call('INCR', client_quota_key)
if new_client_monthly == 1 then
    redis.call('EXPIRE', client_quota_key, 31 * 24 * 60 * 60) -- ~31 days
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
    return { "QUOTA_EXCEEDED", 0, 0, "Client monthly quota limit reached" }
end

-- ============================================================================
-- STEP 5: Check client per-minute rate limit (INCR first, then validate)
-- ============================================================================
local new_client_rate = redis.call('HINCRBY', client_rate_limit_key, 'total', 1)
redis.call('EXPIRE', client_rate_limit_key, 90) -- 90 seconds per-minute window

if new_client_rate > client_rate_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    redis.call('DECR', client_quota_key)
    redis.call('HINCRBY', client_rate_limit_key, 'total', -1)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return { "RATE_LIMIT_EXCEEDED", 0, 0, "Client per-minute rate limit exceeded" }
end

-- ============================================================================
-- STEP 5.5: Check client per-minute bandwidth rate limit
-- ============================================================================
local new_client_bytes_rate = redis.call('HINCRBY', client_rate_limit_key, 'bytes_total', size_bytes)

if new_client_bytes_rate > client_bandwidth_rate_limit then
    redis.call('DECR', app_quota_key)
    redis.call('HINCRBY', app_rate_limit_key, 'total', -1)
    redis.call('DECR', client_quota_key)
    redis.call('HINCRBY', client_rate_limit_key, 'total', -1)
    redis.call('HINCRBY', client_rate_limit_key, 'bytes_total', -size_bytes)
    local current_owner = redis.call('GET', idempotency_key)
    if current_owner == message_id then
        redis.call('DEL', idempotency_key)
    end
    return { "BANDWIDTH_RATE_LIMIT_EXCEEDED", 0, 0, "Client per-minute bandwidth rate limit exceeded" }
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
redis.call('XADD', message_stream_key, '*',
    'message_id', message_id,
    'message_json', message_json,
    'size_bytes', tostring(size_bytes),
    'recipient_client_id', recipient_client_id,
    'application_id', application_id,
    'persist_to_db', tostring(persist_to_db),
    'cache_ttl', '600')        -- 10 minutes for message cache

-- ============================================================================
-- STEP 9: Return success
-- ============================================================================
local remaining = app_monthly_limit - new_app_monthly
return { "OK", 1, remaining, nil }
