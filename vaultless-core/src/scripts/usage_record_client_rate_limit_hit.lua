-- =============================================================================
-- usage_record_client_rate_limit_hit.lua
-- =============================================================================
-- Atomic client metrics recording for rate limit hit events.
--
-- KEYS[1] = counted_key (idempotency)
-- KEYS[2] = hourly_key (client hourly metrics hash)
--
-- ARGV[1] = counted_ttl
-- ARGV[2] = hourly_ttl
--
-- Returns: 1 if counted, 0 if already counted
-- =============================================================================

local counted_key = KEYS[1]
local hourly_key = KEYS[2]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])

local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

local initial_creation = (redis.call('EXISTS', hourly_key) == 0)
redis.call('HINCRBY', hourly_key, 'rate_limit_hits', 1)
if initial_creation then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

return 1
