-- Migration: Add jsonb_merge_patch function
-- Date: 2026-02-05
-- Description: Implements RFC 7396 JSON Merge Patch for PostgreSQL JSONB

-- Function to perform JSON Merge Patch (RFC 7396)
-- This recursively merges two JSONB objects
CREATE OR REPLACE FUNCTION jsonb_merge_patch(target JSONB, patch JSONB)
RETURNS JSONB
LANGUAGE plpgsql
IMMUTABLE
AS $$
BEGIN
    -- If patch is null, return target
    IF patch IS NULL THEN
        RETURN target;
    END IF;

    -- If patch is not an object, replace target with patch
    IF jsonb_typeof(patch) != 'object' THEN
        RETURN patch;
    END IF;

    -- If target is not an object, treat it as empty object
    IF target IS NULL OR jsonb_typeof(target) != 'object' THEN
        target := '{}'::jsonb;
    END IF;

    -- Merge patch into target
    RETURN (
        SELECT jsonb_object_agg(
            COALESCE(t.key, p.key),
            CASE
                -- If patch value is null, remove the key
                WHEN p.value = 'null'::jsonb THEN NULL
                -- If both are objects, recursively merge
                WHEN jsonb_typeof(COALESCE(t.value, '{}'::jsonb)) = 'object'
                     AND jsonb_typeof(p.value) = 'object'
                THEN jsonb_merge_patch(t.value, p.value)
                -- Otherwise use patch value
                WHEN p.key IS NOT NULL THEN p.value
                -- Keep target value if no patch
                ELSE t.value
            END
        )
        FROM jsonb_each(target) t
        FULL OUTER JOIN jsonb_each(patch) p ON t.key = p.key
        WHERE COALESCE(p.value, t.value) IS NOT NULL
          AND COALESCE(p.value, t.value) != 'null'::jsonb
    );
END;
$$;

COMMENT ON FUNCTION jsonb_merge_patch(JSONB, JSONB) IS 'RFC 7396 JSON Merge Patch implementation for PostgreSQL JSONB';
