-- ============================================================================
-- Migration: Add function to get bandwidth quota warnings
-- Description: Create a database function to retrieve applications with 
--              bandwidth quota usage exceeding a specified threshold
-- ============================================================================

BEGIN;

-- Create function to get bandwidth quota warnings
CREATE OR REPLACE FUNCTION get_bandwidth_quota_warnings(
    p_user_id UUID,
    p_threshold DECIMAL DEFAULT 80
)
RETURNS TABLE (
    application_id UUID,
    application_name VARCHAR(255),
    bandwidth_quota_usage_percentage DECIMAL
)
LANGUAGE sql STABLE
AS $$
    SELECT 
        application_id,
        name AS application_name,
        bandwidth_quota_usage_percentage
    FROM mv_applications_with_usage
    WHERE developer_id = p_user_id
        AND bandwidth_quota_usage_percentage >= p_threshold
        AND is_active = true
    ORDER BY bandwidth_quota_usage_percentage DESC;
$$;

COMMIT;