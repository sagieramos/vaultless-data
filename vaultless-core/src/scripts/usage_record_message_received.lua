-- =============================================================================
-- usage_record_message_received.lua
-- =============================================================================
-- Atomic application metrics recording for message received events.
--
-- KEYS[1] = counted_key (idempotency)
-- KEYS[2] = hourly_key (application hourly metrics hash)
--
-- ARGV[1] = counted_ttl
-- ARGV[2] = hourly_ttl
-- ARGV[3] = size_bytes
--
-- Returns: 1 if counted, 0 if already counted
-- =============================================================================

local counted_key = KEYS[1]
local hourly_key = KEYS[2]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])
local size_bytes = tonumber(ARGV[3])

-- Idempotency check
local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

-- Application hourly metrics (hash)
redis.call('HINCRBY', hourly_key, 'messages_received', 1)
redis.call('HINCRBY', hourly_key, 'total_bytes_received', size_bytes)
if redis.call('EXISTS', hourly_key) == 0 then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
