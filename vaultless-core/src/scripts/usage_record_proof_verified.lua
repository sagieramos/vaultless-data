-- =============================================================================
-- usage_record_proof_verified.lua
-- =============================================================================
-- Atomic application metrics recording for proof verified events.
--
-- KEYS[1] = counted_key (idempotency)
-- KEYS[2] = hourly_key (application hourly metrics hash)
-- KEYS[3] = session_proved_key (session proofs verified count)
--
-- ARGV[1] = counted_ttl
-- ARGV[2] = hourly_ttl
-- ARGV[3] = session_ttl
--
-- Returns: 1 if counted, 0 if already counted
-- =============================================================================

local counted_key = KEYS[1]
local hourly_key = KEYS[2]
local session_proved_key = KEYS[3]

local counted_ttl = tonumber(ARGV[1])
local hourly_ttl = tonumber(ARGV[2])
local session_ttl = tonumber(ARGV[3])

-- Idempotency check
local ok = redis.call('SET', counted_key, '1', 'NX', 'EX', counted_ttl)
if not ok then
    return 0
end

-- Application hourly metrics (hash)
redis.call('HINCRBY', hourly_key, 'proofs_verified', 1)
if redis.call('EXISTS', hourly_key) == 0 then
    redis.call('EXPIRE', hourly_key, hourly_ttl)
end

-- Session metrics
redis.call('INCR', session_proved_key)
redis.call('EXPIRE', session_proved_key, session_ttl)

return 1
