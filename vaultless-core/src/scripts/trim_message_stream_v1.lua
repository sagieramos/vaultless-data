-- =============================================================================
-- trim_message_stream_v1.lua (Maintenance Script)
-- =============================================================================
-- Janitor script to trim the message stream to a bounded size.
-- Run periodically (e.g., every 5 minutes) via cron or scheduler.
--
-- luacheck: globals KEYS ARGV redis
--
-- KEY STRUCTURE (1 key):
--   KEYS[1] = Message stream key (e.g., "stream:instant_message:pending")
--
-- ARGUMENTS (1 arg):
--   ARGV[1] = Maximum stream length (entries, e.g., 100000)
--
-- RETURN TABLE:
--   {trimmed_count, remaining_entries, stream_length}
--
-- TRIMMING STRATEGY:
--   Uses XTRIM with MAXLEN ~ (approximate) for efficiency.
--   The ~ provides 10% tolerance to avoid exact trimming on every run.
-- =============================================================================

local stream_key = KEYS[1]
local max_len = tonumber(ARGV[1]) or 100000

-- Get current stream length using XLEN (safer than XINFO indexing)
local length_before = redis.call('XLEN', stream_key)

if length_before == 0 then
    return {0, 0, 0}
end

-- Trim to max_len with ~10% tolerance (avoids exact trimming on every run)
local trimmed = tonumber(redis.call('XTRIM', stream_key, 'MAXLEN', '~', max_len)) or 0

-- Get final length
local length_after = redis.call('XLEN', stream_key)

return {trimmed, length_after, length_before}
