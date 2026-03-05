-- ============================================================================
-- Chart Trends Calculation Function
-- ============================================================================
-- Calculates trend data by comparing current period vs previous period
-- Returns: current_period, previous_period, change_percent, trend_direction

CREATE OR REPLACE FUNCTION calculate_chart_trends(
    p_application_id UUID,
    p_start_date TIMESTAMPTZ,
    p_end_date TIMESTAMPTZ,
    p_granularity TEXT,  -- 'daily' or 'weekly'
    p_metric TEXT       -- 'messages', 'bandwidth', 'storage', 'proofs', 'rate_limits'
)
RETURNS TABLE (
    current_period BIGINT,
    previous_period BIGINT,
    change_percent DOUBLE PRECISION,
    trend_direction TEXT
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_table_name TEXT;
    v_time_column TEXT;
    v_metric_column TEXT;
    v_period_duration INTERVAL;
    v_prev_start TIMESTAMPTZ;
    v_prev_end TIMESTAMPTZ;
    v_current BIGINT;
    v_previous BIGINT;
BEGIN
    -- Determine table and column names
    IF p_granularity = 'weekly' THEN
        v_table_name := 'application_usage_metrics_weekly';
        v_time_column := 'week_start';
    ELSE
        v_table_name := 'application_usage_metrics_daily';
        v_time_column := 'day';
    END IF;

    -- Determine metric column based on requested metric
    CASE p_metric
        WHEN 'messages' THEN
            v_metric_column := 'total_messages_sent + total_messages_received';
        WHEN 'bandwidth' THEN
            v_metric_column := 'total_bytes_sent + total_bytes_received';
        WHEN 'storage' THEN
            v_metric_column := 'total_bytes_stored';
        WHEN 'proofs' THEN
            v_metric_column := 'total_proofs_verified';
        WHEN 'rate_limits' THEN
            v_metric_column := 'total_rate_limit_hits';
        ELSE
            v_metric_column := 'total_messages_sent + total_messages_received';
    END CASE;

    -- Calculate period duration
    v_period_duration := p_end_date - p_start_date;
    v_prev_start := p_start_date - v_period_duration;
    v_prev_end := p_end_date - v_period_duration;

    -- Execute dynamic query to get current and previous period totals
    EXECUTE format(
        $QUERY$
        WITH current_period AS (
            SELECT COALESCE(SUM(%s), 0) as total
            FROM %s m
            WHERE m.application_id = $1
              AND m.%s >= $2
              AND m.%s <= $3
        ),
        previous_period AS (
            SELECT COALESCE(SUM(%s), 0) as total
            FROM %s m
            WHERE m.application_id = $1
              AND m.%s >= $4
              AND m.%s <= $5
        )
        SELECT
            current_period.total,
            previous_period.total
        FROM current_period, previous_period
        $QUERY$,
        v_metric_column, v_table_name, v_time_column, v_time_column,
        v_metric_column, v_table_name, v_time_column, v_time_column
    )
    INTO v_current, v_previous
    USING p_application_id, p_start_date, p_end_date, v_prev_start, v_prev_end;

    -- Calculate percentage change
    IF v_previous > 0 THEN
        change_percent := ((v_current - v_previous)::DOUBLE PRECISION / v_previous::DOUBLE PRECISION) * 100.0;
    ELSIF v_current > 0 THEN
        change_percent := 100.0;
    ELSE
        change_percent := 0.0;
    END IF;

    -- Determine trend direction (5% threshold for "stable")
    IF change_percent > 5.0 THEN
        trend_direction := 'up';
    ELSIF change_percent < -5.0 THEN
        trend_direction := 'down';
    ELSE
        trend_direction := 'stable';
    END IF;

    current_period := v_current;
    previous_period := v_previous;

    RETURN NEXT;
END;
$$;

-- Grant execute permissions
GRANT EXECUTE ON FUNCTION calculate_chart_trends TO vaultless;

-- Add comments
COMMENT ON FUNCTION calculate_chart_trends IS 'Calculates trend data comparing current period vs previous period for chart analytics';
