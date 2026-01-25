-- ============================================================================
-- Migration: Create monthly revenue continuous aggregate
-- Description: TimescaleDB continuous aggregate for efficient monthly revenue
--              computation and charting
-- ============================================================================



-- Create continuous aggregate for monthly revenue by application
CREATE MATERIALIZED VIEW monthly_revenue_by_application
WITH (timescaledb.continuous) AS
SELECT
    application_id,
    time_bucket('1 month', period_start) AS month,
    SUM(estimated_cost_cents) AS total_revenue_cents,
    SUM(messages_sent + messages_received) AS total_messages,
    SUM(total_bytes_sent + total_bytes_received) AS total_bytes,
    SUM(proofs_verified) AS total_proofs,
    COUNT(*) AS billing_records_count,
    AVG(estimated_cost_cents) AS avg_cost_per_record,
    MIN(estimated_cost_cents) AS min_cost_record,
    MAX(estimated_cost_cents) AS max_cost_record
FROM usage_metrics
GROUP BY application_id, time_bucket('1 month', period_start)
WITH NO DATA;

-- Create index for efficient querying
CREATE INDEX idx_monthly_revenue_month_application 
ON monthly_revenue_by_application (month, application_id);



-- Create continuous aggregate for monthly revenue by developer
CREATE MATERIALIZED VIEW monthly_revenue_by_developer
WITH (timescaledb.continuous) AS
SELECT
    a.developer_id,
    time_bucket('1 month', um.period_start) AS month,
    SUM(um.estimated_cost_cents) AS total_revenue_cents,
    COUNT(DISTINCT um.application_id) AS applications_count,
    COUNT(DISTINCT um.api_key_id) AS api_keys_count,
    SUM(um.messages_sent + um.messages_received) AS total_messages,
    SUM(um.total_bytes_sent + um.total_bytes_received) AS total_bytes
FROM usage_metrics um
JOIN applications a ON um.application_id = a.id
GROUP BY a.developer_id, time_bucket('1 month', um.period_start)
WITH NO DATA;

-- Create index for efficient querying
CREATE INDEX idx_monthly_revenue_developer_month
ON monthly_revenue_by_developer (developer_id, month);



-- Create a function to get chart-ready revenue data
CREATE OR REPLACE FUNCTION get_monthly_revenue_chart_data(
    p_application_id UUID DEFAULT NULL,
    p_developer_id UUID DEFAULT NULL,
    p_months_back INTEGER DEFAULT 12
) 
RETURNS TABLE(
    month_label TEXT,
    revenue_cents BIGINT,
    revenue_usd DECIMAL(10,2),
    messages BIGINT,
    bytes_transferred BIGINT
)
LANGUAGE sql STABLE
AS $$
    WITH date_range AS (
        SELECT generate_series(
            date_trunc('month', now()) - ((p_months_back - 1) || ' months')::interval,
            date_trunc('month', now()),
            '1 month'::interval
        ) AS month
    ),
    revenue_data AS (
        SELECT 
            time_bucket('1 month', period_start) AS month,
            SUM(estimated_cost_cents) AS revenue_cents,
            SUM(messages_sent + messages_received) AS messages,
            SUM(total_bytes_sent + total_bytes_received) AS bytes_transferred
        FROM usage_metrics
        WHERE 
            ($1 IS NULL OR application_id = $1)  -- application filter
            AND ($2 IS NULL OR EXISTS (
                SELECT 1 FROM applications a 
                WHERE a.id = usage_metrics.application_id 
                AND a.developer_id = $2
            ))  -- developer filter
            AND period_start >= date_trunc('month', now()) - (($3 - 1) || ' months')::interval
        GROUP BY time_bucket('1 month', period_start)
    )
    SELECT 
        TO_CHAR(dr.month, 'YYYY-MM') AS month_label,
        COALESCE(rd.revenue_cents, 0) AS revenue_cents,
        (COALESCE(rd.revenue_cents, 0) / 100.0)::DECIMAL(10,2) AS revenue_usd,
        COALESCE(rd.messages, 0) AS messages,
        COALESCE(rd.bytes_transferred, 0) AS bytes_transferred
    FROM date_range dr
    LEFT JOIN revenue_data rd ON dr.month = rd.month
    ORDER BY dr.month;
$$;