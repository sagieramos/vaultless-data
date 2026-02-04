-- ============================================================================
-- Migration: Scalable Revenue Rollup Architecture
-- Hierarchy: application_usage_metrics → hourly → daily → monthly
-- ============================================================================

-- ============================================================================
-- 1. HOURLY AGGREGATES (Base layer - only objects reading from application_usage_metrics)
-- ============================================================================

CREATE MATERIALIZED VIEW hourly_revenue_by_application
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    time_bucket('1 hour', period_start) AS hour,
    SUM(messages_sent + messages_received) AS total_messages,
    SUM(total_bytes_sent + total_bytes_received) AS total_bytes,
    SUM(proofs_verified) AS total_proofs,
    COUNT(*) AS records_count
FROM application_usage_metrics
GROUP BY application_id, time_bucket('1 hour', period_start)
WITH NO DATA;

CREATE MATERIALIZED VIEW hourly_revenue_by_developer
WITH (timescaledb.continuous) AS
SELECT
    a.developer_id,
    time_bucket('1 hour', um.period_start) AS hour,
    SUM(um.messages_sent + um.messages_received) AS total_messages,
    SUM(um.total_bytes_sent + um.total_bytes_received) AS total_bytes,
    SUM(um.proofs_verified) AS total_proofs,
    COUNT(DISTINCT um.application_id) AS applications_count
FROM application_usage_metrics um
JOIN applications a ON um.application_id = a.id
GROUP BY a.developer_id, time_bucket('1 hour', um.period_start)
WITH NO DATA;

CREATE INDEX idx_hourly_revenue_app_hour ON hourly_revenue_by_application (application_id, hour);
CREATE INDEX idx_hourly_revenue_dev_hour ON hourly_revenue_by_developer (developer_id, hour);

-- ============================================================================
-- 2. DAILY AGGREGATES (Roll up from hourly)
-- ============================================================================

CREATE MATERIALIZED VIEW daily_revenue_by_application
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    time_bucket('1 day', hour) AS day,
    SUM(total_messages) AS total_messages,
    SUM(total_bytes) AS total_bytes,
    SUM(total_proofs) AS total_proofs,
    SUM(records_count) AS records_count
FROM hourly_revenue_by_application
GROUP BY application_id, time_bucket('1 day', hour)
WITH NO DATA;

CREATE MATERIALIZED VIEW daily_revenue_by_developer
WITH (timescaledb.continuous) AS
SELECT
    developer_id,
    time_bucket('1 day', hour) AS day,
    SUM(total_messages) AS total_messages,
    SUM(total_bytes) AS total_bytes,
    SUM(total_proofs) AS total_proofs,
    MAX(applications_count) AS applications_count
FROM hourly_revenue_by_developer
GROUP BY developer_id, time_bucket('1 day', hour)
WITH NO DATA;

CREATE INDEX idx_daily_revenue_app_day ON daily_revenue_by_application (application_id, day);
CREATE INDEX idx_daily_revenue_dev_day ON daily_revenue_by_developer (developer_id, day);

-- ============================================================================
-- 3. MONTHLY AGGREGATES (Roll up from daily)
-- ============================================================================

CREATE MATERIALIZED VIEW monthly_revenue_by_application
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    time_bucket('1 month', day) AS month,
    SUM(total_messages) AS total_messages,
    SUM(total_bytes) AS total_bytes,
    SUM(total_proofs) AS total_proofs,
    SUM(records_count) AS records_count
FROM daily_revenue_by_application
GROUP BY application_id, time_bucket('1 month', day)
WITH NO DATA;

CREATE MATERIALIZED VIEW monthly_revenue_by_developer
WITH (timescaledb.continuous) AS
SELECT
    developer_id,
    time_bucket('1 month', day) AS month,
    SUM(total_messages) AS total_messages,
    SUM(total_bytes) AS total_bytes,
    SUM(total_proofs) AS total_proofs,
    MAX(applications_count) AS applications_count
FROM daily_revenue_by_developer
GROUP BY developer_id, time_bucket('1 month', day)
WITH NO DATA;

CREATE INDEX idx_monthly_revenue_app_month ON monthly_revenue_by_application (application_id, month);
CREATE INDEX idx_monthly_revenue_dev_month ON monthly_revenue_by_developer (developer_id, month);

-- ============================================================================
-- 4. UNIFIED CHART FUNCTION
-- ============================================================================

CREATE OR REPLACE FUNCTION get_revenue_chart_data(
    p_application_id UUID DEFAULT NULL,
    p_developer_id UUID DEFAULT NULL,
    p_granularity TEXT DEFAULT 'day',
    p_periods_back INTEGER DEFAULT 30
)
RETURNS TABLE(
    period_label TEXT,
    period_start TIMESTAMPTZ,
    revenue_cents BIGINT,
    revenue_usd DECIMAL(12,2),
    messages BIGINT,
    bytes_transferred BIGINT,
    proofs BIGINT
)
LANGUAGE plpgsql STABLE
AS $$
DECLARE
    v_interval INTERVAL;
    v_format TEXT;
    v_start TIMESTAMPTZ;
BEGIN
    CASE p_granularity
        WHEN 'hour' THEN
            v_interval := '1 hour'::INTERVAL;
            v_format := 'YYYY-MM-DD HH24:00';
            v_start := date_trunc('hour', now()) - ((p_periods_back - 1) * v_interval);
        WHEN 'day' THEN
            v_interval := '1 day'::INTERVAL;
            v_format := 'YYYY-MM-DD';
            v_start := date_trunc('day', now()) - ((p_periods_back - 1) * v_interval);
        WHEN 'week' THEN
            v_interval := '1 week'::INTERVAL;
            v_format := 'IYYY-"W"IW';
            v_start := date_trunc('week', now()) - ((p_periods_back - 1) * v_interval);
        WHEN 'month' THEN
            v_interval := '1 month'::INTERVAL;
            v_format := 'YYYY-MM';
            v_start := date_trunc('month', now()) - ((p_periods_back - 1) * v_interval);
        ELSE
            RAISE EXCEPTION 'Invalid granularity: %. Use hour, day, week, or month.', p_granularity;
    END CASE;

    IF p_granularity = 'hour' THEN
        RETURN QUERY
        WITH periods AS (
            SELECT generate_series(v_start, date_trunc('hour', now()), v_interval) AS p
        ),
        agg AS (
            SELECT
                hour AS p,
                SUM(total_messages) AS messages,
                SUM(total_bytes) AS bytes,
                SUM(total_proofs) AS proofs
            FROM hourly_revenue_by_application
            WHERE (p_application_id IS NULL OR application_id = p_application_id)
              AND (p_developer_id IS NULL OR application_id IN (
                  SELECT id FROM applications WHERE developer_id = p_developer_id
              ))
              AND hour >= v_start
            GROUP BY hour
        )
        SELECT
            TO_CHAR(periods.p, v_format),
            periods.p,
            0::BIGINT AS revenue_cents,  -- Placeholder since revenue is now calculated differently
            0.00::DECIMAL(12,2) AS revenue_usd,  -- Placeholder since revenue is now calculated differently
            COALESCE(agg.messages, 0)::BIGINT,
            COALESCE(agg.bytes, 0)::BIGINT,
            COALESCE(agg.proofs, 0)::BIGINT
        FROM periods
        LEFT JOIN agg ON periods.p = agg.p
        ORDER BY periods.p;

    ELSIF p_granularity IN ('day', 'week') THEN
        RETURN QUERY
        WITH periods AS (
            SELECT generate_series(v_start, date_trunc(p_granularity, now()), v_interval) AS p
        ),
        agg AS (
            SELECT
                time_bucket(v_interval, day) AS p,
                SUM(total_messages) AS messages,
                SUM(total_bytes) AS bytes,
                SUM(total_proofs) AS proofs
            FROM daily_revenue_by_application
            WHERE (p_application_id IS NULL OR application_id = p_application_id)
              AND (p_developer_id IS NULL OR application_id IN (
                  SELECT id FROM applications WHERE developer_id = p_developer_id
              ))
              AND day >= v_start
            GROUP BY time_bucket(v_interval, day)
        )
        SELECT
            TO_CHAR(periods.p, v_format),
            periods.p,
            0::BIGINT AS revenue_cents,  -- Placeholder since revenue is now calculated differently
            0.00::DECIMAL(12,2) AS revenue_usd,  -- Placeholder since revenue is now calculated differently
            COALESCE(agg.messages, 0)::BIGINT,
            COALESCE(agg.bytes, 0)::BIGINT,
            COALESCE(agg.proofs, 0)::BIGINT
        FROM periods
        LEFT JOIN agg ON periods.p = agg.p
        ORDER BY periods.p;

    ELSE -- month
        RETURN QUERY
        WITH periods AS (
            SELECT generate_series(v_start, date_trunc('month', now()), v_interval) AS p
        ),
        agg AS (
            SELECT
                month AS p,
                SUM(total_messages) AS messages,
                SUM(total_bytes) AS bytes,
                SUM(total_proofs) AS proofs
            FROM monthly_revenue_by_application
            WHERE (p_application_id IS NULL OR application_id = p_application_id)
              AND (p_developer_id IS NULL OR application_id IN (
                  SELECT id FROM applications WHERE developer_id = p_developer_id
              ))
              AND month >= v_start
            GROUP BY month
        )
        SELECT
            TO_CHAR(periods.p, v_format),
            periods.p,
            0::BIGINT AS revenue_cents,  -- Placeholder since revenue is now calculated differently
            0.00::DECIMAL(12,2) AS revenue_usd,  -- Placeholder since revenue is now calculated differently
            COALESCE(agg.messages, 0)::BIGINT,
            COALESCE(agg.bytes, 0)::BIGINT,
            COALESCE(agg.proofs, 0)::BIGINT
        FROM periods
        LEFT JOIN agg ON periods.p = agg.p
        ORDER BY periods.p;
    END IF;
END;
$$;

-- ============================================================================
-- 5. REFRESH POLICIES
-- ============================================================================

SELECT add_continuous_aggregate_policy('hourly_revenue_by_application',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour');

SELECT add_continuous_aggregate_policy('hourly_revenue_by_developer',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour');

SELECT add_continuous_aggregate_policy('daily_revenue_by_application',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '1 day',
    schedule_interval => INTERVAL '1 day');

SELECT add_continuous_aggregate_policy('daily_revenue_by_developer',
    start_offset => INTERVAL '3 days',
    end_offset => INTERVAL '1 day',
    schedule_interval => INTERVAL '1 day');

SELECT add_continuous_aggregate_policy('monthly_revenue_by_application',
    start_offset => INTERVAL '3 months',
    end_offset => INTERVAL '1 month',
    schedule_interval => INTERVAL '1 day');

SELECT add_continuous_aggregate_policy('monthly_revenue_by_developer',
    start_offset => INTERVAL '3 months',
    end_offset => INTERVAL '1 month',
    schedule_interval => INTERVAL '1 day');

-- CALL refresh_continuous_aggregate('monthly_revenue_by_application', now() - INTERVAL '6 months', now());
-- CALL refresh_continuous_aggregate('monthly_revenue_by_developer', now() - INTERVAL '6 months', now());

