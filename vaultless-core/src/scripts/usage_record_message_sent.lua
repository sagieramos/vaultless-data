-- =============================================================================
-- usage_record_message_sent.lua
-- =============================================================================
-- Atomic application metrics recording for message sent events.
--
-- Performs in a single atomic operation:
-- 1. Idempotency check (prevents double-counting)
-- 2. Application monthly quota increment
-- 3. Application hourly metrics increment
-- 4. TTL setup (only on first creation)
--
-- KEYS[1] = counted_key (idempotency)
-- KEYS[2] = monthly_key (application monthly quota)
-- KEYS[3] = hourly_key (application hourly metrics hash)
--
-- ARGV[1] = counted_ttl
-- ARGV[2] = monthly_ttl
-- ARGV[3] = hourly_ttl
-- ARGV[4] = size_bytes
--
-- Returns: 1 if counted, 0 if already counted
-- =============================================================================

local counted_key = KEYS[1]
local monthly_key = KEYS[2]
local hourly_key = KEYS[3]

local counted_ttl = tonumber(ARGV[1])
local monthly_ttl = tonumber(ARGV[2])
local hourly_ttl = tonumber(ARGV[3])
local size_bytes = tonumber(ARGV[4])

-- Idempotency check: only count once per message
local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

-- Application monthly quota
local monthly_count = redis.call('INCR', monthly_key)
if monthly_count == 1 then
    redis.call('EXPIRE', monthly_key, monthly_ttl)
end

-- Application hourly metrics (hash)
redis.call('HINCRBY', hourly_key, 'messages_sent', 1)
redis.call('HINCRBY', hourly_key, 'total_bytes_sent', size_bytes)
-- Only set TTL on first creation
if redis.call('EXISTS', hourly_key) == 0 then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
