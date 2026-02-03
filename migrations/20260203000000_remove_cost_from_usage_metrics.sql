-- Remove cost-related columns from usage_metrics table and related aggregates
-- Usage metrics should not handle cost calculations; this should be done separately

-- Refresh the continuous aggregate policies to drop them temporarily
CALL refresh_continuous_aggregate('usage_metrics_daily', NULL, NULL);
CALL refresh_continuous_aggregate('usage_metrics_keys_daily', NULL, NULL);

-- Drop the continuous aggregates
DROP MATERIALIZED VIEW usage_metrics_daily;
DROP MATERIALIZED VIEW usage_metrics_keys_daily;

-- Remove the estimated_cost_cents column from the base table
ALTER TABLE usage_metrics DROP COLUMN estimated_cost_cents;

-- Recreate the continuous aggregates without the cost column
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
    SUM(rate_limit_hits)::BIGINT AS total_rate_limit_hits
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

-- Recreate the refresh policies
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