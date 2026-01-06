-- Add expression index for efficient name searches
-- This allows case-insensitive search without scanning all rows first

CREATE INDEX IF NOT EXISTS idx_applications_name_lower
    ON mv_applications_with_usage (developer_id, LOWER(name));
