-- ============================================================================
-- 1. BASE HYPERTABLE: RAW USAGE METRICS
-- ============================================================================
-- We store the raw data here. By including application, subscription, and key,
-- we cover billing, dashboarding, and auditing in one row.

CREATE TABLE usage_metrics (
    -- Primary Time Dimension
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    
    -- Identity Dimensions
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    subscription_id UUID NOT NULL REFERENCES developer_subscriptions(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    
    -- Usage Counters
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

-- Convert to Hypertable (Partitioned by time)
SELECT create_hypertable('usage_metrics', 'period_start', if_not_exists => TRUE);

-- Primary lookup indices for raw data
-- Optimizing for: "Show me usage for this app" or "Show me usage for this specific key"
CREATE INDEX idx_usage_application_lookup ON usage_metrics(application_id, period_start DESC);
CREATE INDEX idx_usage_developer_subscription_lookup ON usage_metrics(subscription_id, period_start DESC);
CREATE INDEX idx_usage_api_key_lookup ON usage_metrics(api_key_id, period_start DESC);

-- Unique constraint: Prevents duplicate writes for the same key in the same window
CREATE UNIQUE INDEX idx_usage_unique_key_period 
ON usage_metrics(api_key_id, application_id, subscription_id, period_start) 
WHERE api_key_id IS NOT NULL;

-- App-level usage (publishable / system)
CREATE UNIQUE INDEX idx_usage_app_period
ON usage_metrics (application_id, subscription_id, period_start)
WHERE api_key_id IS NULL;

-- Compression configuration
-- Segmenting by subscription and application makes cross-app billing queries very fast
ALTER TABLE usage_metrics SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'subscription_id, application_id, api_key_id',
    timescaledb.compress_orderby = 'period_start DESC'
);

SELECT add_compression_policy('usage_metrics', INTERVAL '7 days');
SELECT add_retention_policy('usage_metrics', INTERVAL '90 days');

-- ============================================================================
-- 2. CONTINUOUS AGGREGATES: THE DASHBOARD LAYER
-- ============================================================================

-- Daily App-Level Summary (Powers the main Developer Dashboard)
CREATE MATERIALIZED VIEW usage_metrics_daily
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    subscription_id, -- Keep sub_id here so we can roll up to billing easily
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
GROUP BY application_id, subscription_id, day
WITH NO DATA;

-- Daily API Key Breakdown (Powers the "Troubleshooting/Audit" view)
CREATE MATERIALIZED VIEW usage_metrics_keys_daily
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    api_key_id,
    time_bucket(INTERVAL '1 day', period_start) AS day,
    SUM(messages_sent)::BIGINT AS total_messages_sent,
    SUM(rate_limit_hits)::BIGINT AS total_rate_limit_hits
FROM usage_metrics
WHERE api_key_id IS NOT NULL
GROUP BY application_id, api_key_id, day
WITH NO DATA;

-- ============================================================================
-- 3. REFRESH POLICIES
-- ============================================================================

SELECT add_continuous_aggregate_policy(
    'usage_metrics_daily',
    start_offset => INTERVAL '3 days',
    end_offset   => INTERVAL '0 hours',
    schedule_interval => INTERVAL '1 hour'
);

SELECT add_continuous_aggregate_policy(
    'usage_metrics_keys_daily',
    start_offset => INTERVAL '3 days',
    end_offset   => INTERVAL '0 hours',
    schedule_interval => INTERVAL '1 hour'
);