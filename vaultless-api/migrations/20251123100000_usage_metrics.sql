CREATE TABLE usage_metrics (
    id UUID DEFAULT uuid_generate_v4(),
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    -- RECOMMENDATION: Add api_key_id for auditing. 
    -- Use ON DELETE SET NULL so metrics persist even if a key is deleted/rotated.
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    
    -- Time window
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    
    -- Usage counters
    messages_sent BIGINT NOT NULL DEFAULT 0,
    messages_received BIGINT NOT NULL DEFAULT 0,
    proofs_verified BIGINT NOT NULL DEFAULT 0,
    total_bytes_stored BIGINT NOT NULL DEFAULT 0,
    total_bytes_sent BIGINT NOT NULL DEFAULT 0,
    total_bytes_received BIGINT NOT NULL DEFAULT 0,
    
    -- Rate limiting violations
    rate_limit_hits INTEGER NOT NULL DEFAULT 0,
    
    -- Cost tracking (for billing)
    estimated_cost_cents BIGINT DEFAULT 0,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT valid_period CHECK (period_end > period_start),
    CONSTRAINT valid_counters CHECK (
        messages_sent >= 0 AND 
        messages_received >= 0 AND 
        proofs_verified >= 0 AND
        total_bytes_stored >= 0 AND
        total_bytes_sent >= 0 AND
        total_bytes_received >= 0 AND
        rate_limit_hits >= 0
    )
);

-- ============================================================================
-- HYPERTABLE CONFIGURATION
-- ============================================================================

SELECT create_hypertable('usage_metrics', 'period_start', if_not_exists => TRUE);

-- Primary lookup indices
CREATE INDEX idx_usage_application_id ON usage_metrics(application_id, period_start DESC);
CREATE INDEX idx_usage_api_key_id ON usage_metrics(api_key_id, period_start DESC);

-- Unique constraint: Allows one record per KEY per HOUR.
-- This allows you to see which specific key was used during a rotation overlap.
CREATE UNIQUE INDEX idx_usage_unique_key_period 
ON usage_metrics(api_key_id, period_start) 
WHERE api_key_id IS NOT NULL;

-- Compression configuration
ALTER TABLE usage_metrics SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'application_id, api_key_id'
);

SELECT add_compression_policy('usage_metrics', INTERVAL '7 days');
SELECT add_retention_policy('usage_metrics', INTERVAL '90 days');

-- ============================================================================
-- CONTINUOUS AGGREGATES: The "Quota" Layer
-- ============================================================================
-- We group ONLY by application_id here. 
-- This automatically merges usage from old and new keys into one application total.

-- Daily Summary
CREATE MATERIALIZED VIEW usage_metrics_daily
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    time_bucket(INTERVAL '1 day', period_start) AS day,
    SUM(messages_sent)::BIGINT AS total_messages_sent,
    SUM(messages_received)::BIGINT AS total_messages_received,
    SUM(proofs_verified)::BIGINT AS total_proofs_verified,
    SUM(total_bytes_stored)::BIGINT AS total_bytes_stored,
    SUM(total_bytes_sent)::BIGINT AS total_bytes_sent,
    SUM(total_bytes_received)::BIGINT AS total_bytes_received,
    SUM(rate_limit_hits)::BIGINT AS total_rate_limit_hits,
    SUM(estimated_cost_cents)::BIGINT AS total_estimated_cost_cents
FROM usage_metrics
GROUP BY application_id, day
WITH NO DATA;

SELECT add_continuous_aggregate_policy(
    'usage_metrics_daily',
    start_offset => INTERVAL '3 days',
    end_offset   => INTERVAL '0 hours',
    schedule_interval => INTERVAL '1 hour'
);

-- Weekly Summary
CREATE MATERIALIZED VIEW usage_metrics_weekly
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    time_bucket(INTERVAL '7 days', period_start) AS week_start,
    SUM(messages_sent)::BIGINT AS total_messages_sent,
    SUM(messages_received)::BIGINT AS total_messages_received,
    SUM(proofs_verified)::BIGINT AS total_proofs_verified,
    SUM(total_bytes_sent)::BIGINT AS total_bytes_sent,
    SUM(total_bytes_received)::BIGINT AS total_bytes_received,
    SUM(rate_limit_hits)::BIGINT AS total_rate_limit_hits,
    SUM(estimated_cost_cents)::BIGINT AS total_estimated_cost_cents
FROM usage_metrics
GROUP BY application_id, week_start
WITH NO DATA;

SELECT add_continuous_aggregate_policy(
    'usage_metrics_weekly',
    start_offset => INTERVAL '1 month',
    end_offset   => INTERVAL '1 day',
    schedule_interval => INTERVAL '1 day'
);