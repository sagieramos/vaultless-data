-- Create weekly continuous aggregate for application usage metrics
-- This provides a coarser granularity for long-term trend analysis

-- Weekly App-Level Summary
CREATE MATERIALIZED VIEW IF NOT EXISTS application_usage_metrics_weekly
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    subscription_id,
    time_bucket(INTERVAL '1 week', period_start) AS week_start,
    SUM(messages_sent)::BIGINT AS total_messages_sent,
    SUM(messages_received)::BIGINT AS total_messages_received,
    SUM(proofs_verified)::BIGINT AS total_proofs_verified,
    SUM(total_bytes_stored)::BIGINT AS total_bytes_stored,
    SUM(total_bytes_sent)::BIGINT AS total_bytes_sent,
    SUM(total_bytes_received)::BIGINT AS total_bytes_received,
    SUM(rate_limit_hits)::BIGINT AS total_rate_limit_hits
FROM application_usage_metrics
GROUP BY application_id, subscription_id, week_start
WITH NO DATA;

-- Add refresh policy for weekly aggregate
SELECT add_continuous_aggregate_policy(
    'application_usage_metrics_weekly',
    start_offset => INTERVAL '3 weeks',
    end_offset   => INTERVAL '0 weeks',
    schedule_interval => INTERVAL '1 day'
);
