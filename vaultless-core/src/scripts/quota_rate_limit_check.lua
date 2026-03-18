-- Combined quota and rate limit validation script
-- Optimized for high throughput: checks cheap operations first (rate limit) before expensive ones (quota)
-- KEYS[1]: rate_limit_key (e.g., "metric:sk:<sk_id>:minute:<timestamp>")
-- KEYS[2]: period_quota_key (e.g., "quota:app:<app_id>")
-- ARGV[1]: rate_limit_per_minute
-- ARGV[2]: period_quota_limit
-- Returns: {status, period_usage} where status:
--   1 = OK
--   2 = QUOTA_EXCEEDED
--   3 = RATE_LIMIT_EXCEEDED

local rate_limit_key = KEYS[1]
local period_quota_key = KEYS[2]
local rate_limit_per_minute = tonumber(ARGV[1])
local period_quota_limit = tonumber(ARGV[2])

-- Check and increment rate limit FIRST (cheap + protects system early)
local current = redis.call("INCR", rate_limit_key)

-- Set TTL on first request in the window (prevents memory leaks)
if current == 1 then
    redis.call("EXPIRE", rate_limit_key, 60)  -- 60 second window
end

-- Fail fast on rate limit exceeded
if current > rate_limit_per_minute then
    -- Still fetch current usage for telemetry
    local period_usage = redis.call("GET", period_quota_key) or 0
    return {3, tonumber(period_usage)}
end

-- Check period quota (more expensive operation)
local period_usage = redis.call("GET", period_quota_key)
if period_usage then
    period_usage = tonumber(period_usage)
else
    period_usage = 0
end

if period_usage >= period_quota_limit then
    return {2, period_usage}
end

-- Both checks passed
return {1, period_usage}