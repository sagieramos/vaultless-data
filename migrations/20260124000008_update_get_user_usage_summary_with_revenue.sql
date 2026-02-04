-- ============================================================================
-- Migration: Update get_user_usage_summary function to include revenue
-- Description: Add total monthly revenue to the function that returns user 
--              usage summary information
-- ============================================================================

BEGIN;

-- Drop the existing function with the old signature
DROP FUNCTION IF EXISTS get_user_usage_summary(uuid);

-- Create the function with the new signature that includes revenue
CREATE FUNCTION get_user_usage_summary(p_developer_id uuid)
RETURNS TABLE(
    total_apps integer,
    total_monthly_messages bigint,
    total_clients bigint,
    critical_quota_apps integer,
    critical_bandwidth_quota_apps integer,
    total_monthly_revenue_cents bigint
)
LANGUAGE sql STABLE
AS $$
    SELECT
        COUNT(*)::INTEGER,
        COALESCE(SUM(current_month_messages_sent), 0)::BIGINT,
        COALESCE(SUM(client_count), 0)::BIGINT,
        COUNT(*) FILTER (WHERE quota_usage_percentage >= 90)::INTEGER,
        COUNT(*) FILTER (WHERE bandwidth_quota_usage_percentage >= 90)::INTEGER,
        COALESCE(SUM(current_month_revenue_cents), 0)::BIGINT
    FROM mv_applications_with_usage
    WHERE developer_id = p_developer_id;
$$;

COMMIT;