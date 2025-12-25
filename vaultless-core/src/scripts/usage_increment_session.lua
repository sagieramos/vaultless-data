-- =============================================================================
-- usage_increment_session.lua
-- =============================================================================
-- Lua script for incrementing session counters only (no idempotency).
-- Used for background/cached updates where counts are already known.
--
-- KEYS[1] = sent_key
-- KEYS[2] = bytes_sent_key
-- KEYS[3] = rcvd_key
-- KEYS[4] = bytes_rcvd_key
-- KEYS[5] = proved_key
--
-- ARGV[1] = sent_delta
-- ARGV[2] = bytes_sent_delta
-- ARGV[3] = rcvd_delta
-- ARGV[4] = bytes_rcvd_delta
-- ARGV[5] = proved_delta
-- ARGV[6] = ttl
--
-- Returns: 1 on success
-- =============================================================================

local sent_key = KEYS[1]
local bytes_sent_key = KEYS[2]
local rcvd_key = KEYS[3]
local bytes_rcvd_key = KEYS[4]
local proved_key = KEYS[5]

local sent_delta = tonumber(ARGV[1])
local bytes_sent_delta = tonumber(ARGV[2])
local rcvd_delta = tonumber(ARGV[3])
local bytes_rcvd_delta = tonumber(ARGV[4])
local proved_delta = tonumber(ARGV[5])
local ttl = tonumber(ARGV[6])

if sent_delta > 0 then
    redis.call('INCRBY', sent_key, sent_delta)
    redis.call('EXPIRE', sent_key, ttl)
end
if bytes_sent_delta > 0 then
    redis.call('INCRBY', bytes_sent_key, bytes_sent_delta)
    redis.call('EXPIRE', bytes_sent_key, ttl)
end
if rcvd_delta > 0 then
    redis.call('INCRBY', rcvd_key, rcvd_delta)
    redis.call('EXPIRE', rcvd_key, ttl)
end
if bytes_rcvd_delta > 0 then
    redis.call('INCRBY', bytes_rcvd_key, bytes_rcvd_delta)
    redis.call('EXPIRE', bytes_rcvd_key, ttl)
end
if proved_delta > 0 then
    redis.call('INCRBY', proved_key, proved_delta)
    redis.call('EXPIRE', proved_key, ttl)
end

return 1
