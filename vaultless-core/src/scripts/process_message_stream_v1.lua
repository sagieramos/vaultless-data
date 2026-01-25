-- =============================================================================
-- process_message_stream_v1.lua (Warm Path Worker)
-- =============================================================================
-- Async worker script for processing pending messages from Redis Stream.
-- Consumes messages via consumer groups, stores in cache, enqueues to inbox,
-- and tracks recipient bandwidth metrics.
-- Designed to be called by multiple parallel workers.
--
-- luacheck: globals KEYS ARGV redis cjson
--
-- KEY STRUCTURE (6 keys):
--   KEYS[1] = Message stream key (e.g., "stream:instant_message:pending")
--   KEYS[2] = Message cache key (e.g., "instant_message:message:{msg_id}")
--   KEYS[3] = Recipient inbox queue key (e.g., "instant_message:inbox:{recipient_client_id}")
--   KEYS[4] = Dead letter queue key (e.g., "stream:instant_message:dlq")
--   KEYS[5] = Client active keys set (e.g., "metric:client:active_keys")
--   KEYS[6] = App active clients key base (e.g., "metric:app:{app_id}:active_clients")
--
-- ARGUMENTS (8 args):
--   ARGV[1] = Message cache TTL (seconds, 600)
--   ARGV[2] = Inbox max length (keep last N messages)
--   ARGV[3] = Consumer group name
--   ARGV[4] = Consumer name (worker identifier)
--   ARGV[5] = Client metric TTL (seconds, ~7 days)
--   ARGV[6] = Active clients set TTL (seconds, 24 hours)
--   ARGV[7] = Delivery idempotency TTL (seconds, 1 hour)
--   ARGV[8] = App metric TTL (seconds, ~2 hours)
--
-- RETURN TABLE:
--   {processed, error_count, stream_position}
--
-- FLOW:
--   1. Read pending messages from consumer group (blocking, ~5s timeout)
--   2. For each message:
--      a. Store in message cache with TTL
--      b. Enqueue to recipient inbox (bounded queue)
--      c. Track recipient bandwidth metrics (messages_received, total_bytes_received)
--      d. Acknowledge message via consumer group
--   3. On failure: move to DLQ after max retries
--
-- CONSUMER GROUP SETUP (run once):
--   XGROUP CREATE stream:instant_message:pending msg_processors MKSTREAM
-- =============================================================================

local stream_key = KEYS[1]
local message_cache_key_base = KEYS[2]
local inbox_queue_key_base = KEYS[3]
local dlq_key = KEYS[4]
local client_active_keys_set = KEYS[5]
local app_active_clients_key_base = KEYS[6]

local cache_ttl = tonumber(ARGV[1])
local inbox_max_len = tonumber(ARGV[2])
local group_name = ARGV[3]
local consumer_name = ARGV[4]
local client_metric_ttl = tonumber(ARGV[5])
local active_clients_ttl = tonumber(ARGV[6])
local delivery_idempotency_ttl = tonumber(ARGV[7])
local app_metric_ttl = tonumber(ARGV[8])

-- ============================================================================
-- STEP 0: Ensure consumer group exists (safe creation without destroying)
-- ============================================================================
-- Use pcall to safely create group if it doesn't exist
-- Never destroy groups in worker code to preserve pending messages
local ok, _ = pcall(function()
    redis.call('XGROUP', 'CREATE', stream_key, group_name, '0', 'MKSTREAM')
end)
-- Group creation is fire-and-forget; if it exists, we proceed anyway

-- ============================================================================
-- STEP 1: Read pending messages from consumer group
-- ============================================================================
local messages = redis.call('XREADGROUP', 'GROUP', group_name, consumer_name,
                            'COUNT', 10, 'BLOCK', 5000,
                            'STREAMS', stream_key, '>')

-- If no messages, return empty result
if not messages or #messages == 0 then
    return {0, 0, nil}
end

local stream_info = messages[1]
local stream_name = stream_info[1]
local message_list = stream_info[2]

local processed = 0
local error_count = 0
local last_id = nil

-- ============================================================================
-- STEP 2: Process each message
-- ============================================================================
for i = 1, #message_list do
    local entry = message_list[i]
    local entry_id = entry[1]
    local fields = entry[2]

    -- Parse stream fields
    local message_id = nil
    local message_json = nil
    local size_bytes = nil
    local recipient_client_id = nil
    local application_id = nil
    local actual_cache_ttl = nil

    for j = 1, #fields, 2 do
        local key = fields[j]
        local value = fields[j + 1]
        if key == 'message_id' then
            message_id = value
        elseif key == 'message_json' then
            message_json = value
        elseif key == 'size_bytes' then
            size_bytes = tonumber(value)
        elseif key == 'recipient_client_id' then
            recipient_client_id = value
        elseif key == 'application_id' then
            application_id = value
        elseif key == 'cache_ttl' then
            actual_cache_ttl = tonumber(value)
        end
    end

    -- Validate required fields
    if not message_id or not message_json or not recipient_client_id then
        redis.call('XADD', dlq_key, '*',
                   'entry_id', entry_id,
                   'reason', 'missing_fields',
                   'error', 'Required fields missing')
        redis.call('XACK', stream_key, group_name, entry_id)
        error_count = error_count + 1
        last_id = entry_id
    else
        -- Construct dynamic keys
        local cache_key = string.gsub(message_cache_key_base, '{msg_id}', message_id)
        local inbox_key = string.gsub(inbox_queue_key_base, '{recipient_client_id}', recipient_client_id)
        local ttl_to_use = actual_cache_ttl or cache_ttl

        -- Process message (store + enqueue + track recipient metrics) with proper error capture
        local success, err_msg = pcall(function()
            -- Store message in cache
            redis.call('SET', cache_key, message_json, 'EX', ttl_to_use)

            -- Enqueue to recipient inbox (bounded queue)
            redis.call('RPUSH', inbox_key, message_json)
            redis.call('LTRIM', inbox_key, -inbox_max_len, -1)

            -- ================================================================
            -- Track recipient bandwidth metrics (messages_received, bytes_received)
            -- ================================================================
            if application_id and size_bytes then
                -- Delivery idempotency key to prevent double-counting
                local delivery_key = 'delivered:msg:' .. message_id .. ':' .. recipient_client_id
                local was_delivered = redis.call('SET', delivery_key, '1', 'NX', 'EX', delivery_idempotency_ttl)

                if was_delivered then
                    -- Get current hour window for metric keys
                    local now = redis.call('TIME')
                    local timestamp = tonumber(now[1])
                    local hour_ts = math.floor(timestamp / 3600) * 3600
                    local dt = os.date('!*t', hour_ts)
                    local hour_suffix = string.format('%04d_%02d_%02d_%02d', dt.year, dt.month, dt.day, dt.hour)

                    -- Recipient client metric key
                    local recipient_metric_key = 'metric:client:' .. application_id .. ':' .. recipient_client_id .. ':hour:' .. hour_suffix

                    -- Track recipient in active keys set (for flusher)
                    local is_new_key = redis.call('EXISTS', recipient_metric_key) == 0
                    if is_new_key then
                        redis.call('EXPIRE', recipient_metric_key, client_metric_ttl)
                    end
                    redis.call('SADD', client_active_keys_set, recipient_metric_key)

                    -- Increment recipient metrics
                    redis.call('HINCRBY', recipient_metric_key, 'messages_received', 1)
                    redis.call('HINCRBY', recipient_metric_key, 'total_bytes_received', size_bytes)

                    -- Track recipient in per-app active clients set
                    local app_active_clients_key = string.gsub(app_active_clients_key_base, '{app_id}', application_id)
                    redis.call('SADD', app_active_clients_key, recipient_client_id)
                    redis.call('EXPIRE', app_active_clients_key, active_clients_ttl)

                    -- Application-level received metrics
                    local app_metric_key = 'metric:app:' .. application_id .. ':hour:' .. hour_suffix
                    local is_new_app_key = redis.call('EXISTS', app_metric_key) == 0
                    if is_new_app_key then
                        redis.call('EXPIRE', app_metric_key, app_metric_ttl)
                    end
                    redis.call('HINCRBY', app_metric_key, 'messages_received', 1)
                    redis.call('HINCRBY', app_metric_key, 'total_bytes_received', size_bytes)
                end
            end
        end)

        if success then
            -- Acknowledge successful processing
            redis.call('XACK', stream_key, group_name, entry_id)
            processed = processed + 1
        else
            -- On error, move to DLQ with error details for diagnostics
            redis.call('XADD', dlq_key, '*',
                       'entry_id', entry_id,
                       'message_id', message_id or 'unknown',
                       'reason', 'processing_error',
                       'error', tostring(err_msg),
                       'retry_count', '1')
            redis.call('XACK', stream_key, group_name, entry_id)
            error_count = error_count + 1
        end
        last_id = entry_id
    end
end

-- ============================================================================
-- STEP 3: Return processing stats
-- ============================================================================
-- Note: Stream trimming is handled by a separate maintenance process
-- to avoid performance impact during message processing.
-- ============================================================================
return {processed, error_count, last_id}
