-- =============================================================================
-- process_message_stream_v1.lua (Warm Path Worker)
-- =============================================================================
-- Async worker script for processing pending messages from Redis Stream.
-- Consumes messages via consumer groups, stores in cache, and enqueues to inbox.
-- Designed to be called by multiple parallel workers.
--
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (4 keys):
--   KEYS[1] = Message stream key (e.g., "stream:instant_message:pending")
--   KEYS[2] = Message cache key (e.g., "instant_message:message:{msg_id}")
--   KEYS[3] = Recipient inbox queue key (e.g., "instant_message:inbox:{recipient_client_id}")
--   KEYS[4] = Dead letter queue key (e.g., "stream:instant_message:dlq")
--
-- ARGUMENTS (4 args):
--   ARGV[1] = Message cache TTL (seconds, 600)
--   ARGV[2] = Inbox max length (keep last N messages)
--   ARGV[3] = Consumer group name
--   ARGV[4] = Consumer name (worker identifier)
--
-- RETURN TABLE:
--   {processed, error_count, stream_position}
--
-- FLOW:
--   1. Read pending messages from consumer group (blocking, ~5s timeout)
--   2. For each message:
--      a. Store in message cache with TTL
--      b. Enqueue to recipient inbox (bounded queue)
--      c. Acknowledge message via consumer group
--   3. On failure: move to DLQ after max retries
--
-- CONSUMER GROUP SETUP (run once):
--   XGROUP CREATE stream:instant_message:pending msg_processors MKSTREAM
-- =============================================================================

local stream_key = KEYS[1]
local message_cache_key_base = KEYS[2]
local inbox_queue_key_base = KEYS[3]
local dlq_key = KEYS[4]

local cache_ttl = tonumber(ARGV[1])
local inbox_max_len = tonumber(ARGV[2])
local group_name = ARGV[3]
local consumer_name = ARGV[4]

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

        -- Process message (store + enqueue) with proper error capture
        local success, err_msg = pcall(function()
            -- Store message in cache
            redis.call('SET', cache_key, message_json, 'EX', ttl_to_use)

            -- Enqueue to recipient inbox (bounded queue)
            redis.call('RPUSH', inbox_key, message_json)
            redis.call('LTRIM', inbox_key, -inbox_max_len, -1)
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
