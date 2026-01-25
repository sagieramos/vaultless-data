-- =============================================================================
-- record_delivery_v1.lua (Warm Path - Recipient Metrics)
-- =============================================================================
-- Called by warm path workers when a message is delivered to recipient's inbox.
-- Tracks recipient-side metrics: messages_received, total_bytes_received.
-- Also tracks application-level received bandwidth.
--
-- This complements send_message_v2.lua which tracks sender-side metrics.
-- Together they provide complete bandwidth tracking:
--   - Sender: messages_sent, total_bytes_sent (hot path)
--   - Recipient: messages_received, total_bytes_received (warm path - this script)
--
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (5 keys):
--   KEYS[1] = Recipient client metric key (hash, e.g., "metric:client:{app_id}:{client_id}:hour:{window}")
--   KEYS[2] = Client active keys set (e.g., "metric:client:active_keys")
--   KEYS[3] = App active clients set (e.g., "metric:app:{app_id}:active_clients")
--   KEYS[4] = Delivery idempotency key (e.g., "delivered:msg:{msg_id}:{recipient_id}")
--   KEYS[5] = Application hourly metric key (e.g., "metric:app:{app_id}:hour:{window}")
--
-- ARGUMENTS (6 args):
--   ARGV[1] = Message size in bytes
--   ARGV[2] = Recipient client ID
--   ARGV[3] = Client metric TTL (seconds, ~7 days)
--   ARGV[4] = Active clients set TTL (seconds, 24 hours)
--   ARGV[5] = Idempotency TTL (seconds, 1 hour)
--   ARGV[6] = Application metric TTL (seconds, ~2 hours)
--
-- RETURN:
--   1 = Success, metrics recorded
--   0 = Duplicate delivery (already recorded)
-- =============================================================================

local recipient_metric_key = KEYS[1]
local client_active_keys_set = KEYS[2]
local app_active_clients_set = KEYS[3]
local delivery_idempotency_key = KEYS[4]
local app_metric_key = KEYS[5]

local size_bytes = tonumber(ARGV[1])
local recipient_client_id = ARGV[2]
local client_metric_ttl = tonumber(ARGV[3])
local active_clients_ttl = tonumber(ARGV[4])
local idempotency_ttl = tonumber(ARGV[5])
local app_metric_ttl = tonumber(ARGV[6])

-- ============================================================================
-- STEP 1: Idempotency check - ensure we don't double-count deliveries
-- ============================================================================
local was_set = redis.call('SET', delivery_idempotency_key, '1', 'NX', 'EX', idempotency_ttl)
if not was_set then
    return 0  -- Already delivered, skip
end

-- ============================================================================
-- STEP 2: Increment recipient client metrics
-- ============================================================================
local is_new_client_key = redis.call('EXISTS', recipient_metric_key) == 0
if is_new_client_key then
    redis.call('EXPIRE', recipient_metric_key, client_metric_ttl)
end

-- Track recipient in active keys set (for flusher)
redis.call('SADD', client_active_keys_set, recipient_metric_key)

-- Track recipient in per-app active clients set
redis.call('SADD', app_active_clients_set, recipient_client_id)
redis.call('EXPIRE', app_active_clients_set, active_clients_ttl)

-- Increment recipient metrics
redis.call('HINCRBY', recipient_metric_key, 'messages_received', 1)
redis.call('HINCRBY', recipient_metric_key, 'total_bytes_received', size_bytes)

-- ============================================================================
-- STEP 3: Increment application-level received metrics
-- ============================================================================
local is_new_app_key = redis.call('EXISTS', app_metric_key) == 0
if is_new_app_key then
    redis.call('EXPIRE', app_metric_key, app_metric_ttl)
end

-- Increment application received metrics
redis.call('HINCRBY', app_metric_key, 'messages_received', 1)
redis.call('HINCRBY', app_metric_key, 'total_bytes_received', size_bytes)

return 1
