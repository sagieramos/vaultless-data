-- Migration: Enforce one secret key per application + add retrieval view
-- SQLx migration file: migrations/{timestamp}_enforce_secret_key_constraint.sql

-- ============================================================================
-- 1. Add UNIQUE PARTIAL INDEX to enforce exactly one secret key per application
-- ============================================================================
-- This is the most reliable way to enforce "exactly one" in Postgres
-- It allows NULL application_ids and ensures only one secret key per non-NULL application_id

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_one_secret_per_app
    ON public.api_keys (application_id)
    WHERE key_type = 'secret' AND is_active = true;

COMMENT ON INDEX idx_api_keys_one_secret_per_app IS 
    'Ensures exactly one active secret key exists per application at database level';


-- ============================================================================
-- 2. Remove the trigger that limits publishable keys (if it exists)
-- ============================================================================
-- Since you want unlimited publishable keys with rotation capability

DROP TRIGGER IF EXISTS trigger_check_max_publishable_keys ON public.api_keys;

COMMENT ON TABLE public.api_keys IS 
    'API keys table: One secret key per application (enforced by partial unique index), unlimited publishable keys for rotation';


-- ============================================================================
-- 3. Create function to retrieve application with keys
-- ============================================================================
-- Returns application details + array of publishable keys + secret_key_id

CREATE OR REPLACE FUNCTION public.get_application_with_keys(p_application_id UUID)
RETURNS TABLE (
    -- Application fields
    application_id UUID,
    user_id UUID,
    name VARCHAR(255),
    description TEXT,
    is_active BOOLEAN,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ,
    max_ttl_seconds INTEGER,
    is_key_rotation_forced BOOLEAN,
    deletion_requested_at TIMESTAMPTZ,
    internal_notes TEXT,
    integrity_config JSONB,
    
    -- Secret key reference (ID only, never the actual key)
    secret_key_id UUID,
    
    -- Publishable keys array (full details for client use)
    publishable_keys JSONB
)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
AS $$
BEGIN
    RETURN QUERY
    SELECT 
        -- Application columns
        a.id AS application_id,
        a.user_id,
        a.name,
        a.description,
        a.is_active,
        a.created_at,
        a.updated_at,
        a.max_ttl_seconds,
        a.is_key_rotation_forced,
        a.deletion_requested_at,
        a.internal_notes,
        a.integrity_config,
        
        -- Secret key ID (for internal server use only)
        sk.id AS secret_key_id,
        
        -- Publishable keys as JSONB array
        COALESCE(
            (
                SELECT jsonb_agg(
                    jsonb_build_object(
                        'id', pk.id,
                        'key_prefix', pk.key_prefix,
                        'publishable_key_plaintext', pk.publishable_key_plaintext,
                        'description', pk.description,
                        'is_active', pk.is_active,
                        'created_at', pk.created_at,
                        'expires_at', pk.expires_at,
                        'last_used_at', pk.last_used_at
                    )
                    ORDER BY pk.created_at DESC
                )
                FROM public.api_keys pk
                WHERE pk.application_id = p_application_id
                  AND pk.key_type = 'publishable'
                  AND pk.is_active = true
            ),
            '[]'::jsonb
        ) AS publishable_keys
        
    FROM public.applications a
    
    -- LEFT JOIN to get the secret key ID (should always exist due to business logic)
    LEFT JOIN public.api_keys sk ON (
        sk.application_id = a.id 
        AND sk.key_type = 'secret' 
        AND sk.is_active = true
    )
    
    WHERE a.id = p_application_id;
END;
$$;

COMMENT ON FUNCTION public.get_application_with_keys(UUID) IS 
    'Retrieves application details with secret_key_id (for internal use) and publishable_keys array (for API responses)';


-- ============================================================================
-- 4. Create materialized view for efficient multi-application queries (optional)
-- ============================================================================
-- Use this if you need to list many applications with their keys at once

CREATE MATERIALIZED VIEW IF NOT EXISTS public.mv_applications_with_keys AS
SELECT 
    a.id AS application_id,
    a.user_id,
    a.name,
    a.description,
    a.is_active,
    a.created_at,
    a.updated_at,
    a.max_ttl_seconds,
    a.is_key_rotation_forced,
    a.deletion_requested_at,
    a.integrity_config,
    
    -- Secret key ID
    sk.id AS secret_key_id,
    
    -- Count of active publishable keys
    COUNT(pk.id) FILTER (WHERE pk.key_type = 'publishable' AND pk.is_active = true) AS publishable_key_count,
    
    -- Publishable keys as JSONB array
    COALESCE(
        jsonb_agg(
            jsonb_build_object(
                'id', pk.id,
                'key_prefix', pk.key_prefix,
                'publishable_key_plaintext', pk.publishable_key_plaintext,
                'description', pk.description,
                'is_active', pk.is_active,
                'created_at', pk.created_at,
                'expires_at', pk.expires_at
            )
            ORDER BY pk.created_at DESC
        ) FILTER (WHERE pk.key_type = 'publishable' AND pk.is_active = true),
        '[]'::jsonb
    ) AS publishable_keys

FROM public.applications a

LEFT JOIN public.api_keys sk ON (
    sk.application_id = a.id 
    AND sk.key_type = 'secret' 
    AND sk.is_active = true
)

LEFT JOIN public.api_keys pk ON (
    pk.application_id = a.id 
    AND pk.key_type = 'publishable' 
    AND pk.is_active = true
)

GROUP BY 
    a.id, a.user_id, a.name, a.description, a.is_active, 
    a.created_at, a.updated_at, a.max_ttl_seconds, 
    a.is_key_rotation_forced, a.deletion_requested_at, 
    a.integrity_config, sk.id;

-- Index for efficient user lookups
CREATE UNIQUE INDEX IF NOT EXISTS idx_mv_applications_with_keys_app_id 
    ON public.mv_applications_with_keys (application_id);

CREATE INDEX IF NOT EXISTS idx_mv_applications_with_keys_user_id 
    ON public.mv_applications_with_keys (user_id);

COMMENT ON MATERIALIZED VIEW public.mv_applications_with_keys IS 
    'Materialized view for efficient bulk queries of applications with their keys. Refresh periodically or on-demand.';


-- ============================================================================
-- 5. Create helper function to refresh the materialized view
-- ============================================================================

CREATE OR REPLACE FUNCTION public.refresh_applications_with_keys_view()
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY public.mv_applications_with_keys;
END;
$$;

COMMENT ON FUNCTION public.refresh_applications_with_keys_view() IS 
    'Refreshes the applications_with_keys materialized view. Call after bulk key changes.';


-- ============================================================================
-- 6. Add indexes for optimal query performance
-- ============================================================================

-- Index for finding secret keys by application (redundant with unique index but explicit)
CREATE INDEX IF NOT EXISTS idx_api_keys_secret_lookup
    ON public.api_keys (application_id, key_type)
    WHERE key_type = 'secret' AND is_active = true;

-- Index for finding publishable keys by application
CREATE INDEX IF NOT EXISTS idx_api_keys_publishable_lookup
    ON public.api_keys (application_id, created_at DESC)
    WHERE key_type = 'publishable' AND is_active = true;

COMMENT ON INDEX idx_api_keys_publishable_lookup IS 
    'Optimizes retrieval of publishable keys sorted by creation date (most recent first)';


-- ============================================================================
-- 7. Add validation trigger to ensure secret keys have required quota fields
-- ============================================================================

CREATE OR REPLACE FUNCTION public.validate_secret_key_constraints()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    -- Ensure secret keys have application_id
    IF NEW.key_type = 'secret' AND NEW.application_id IS NULL THEN
        RAISE EXCEPTION 'Secret keys must be associated with an application';
    END IF;
    
    -- Ensure secret keys have quota settings
    IF NEW.key_type = 'secret' THEN
        IF NEW.monthly_message_quota IS NULL OR NEW.monthly_message_quota <= 0 THEN
            RAISE EXCEPTION 'Secret keys must have a valid monthly_message_quota';
        END IF;
        
        IF NEW.message_retention_seconds IS NULL OR NEW.message_retention_seconds <= 0 THEN
            RAISE EXCEPTION 'Secret keys must have a valid message_retention_seconds';
        END IF;
    END IF;
    
    RETURN NEW;
END;
$$;

CREATE TRIGGER trigger_validate_secret_key_constraints
    BEFORE INSERT OR UPDATE ON public.api_keys
    FOR EACH ROW
    WHEN (NEW.key_type = 'secret')
    EXECUTE FUNCTION public.validate_secret_key_constraints();

COMMENT ON FUNCTION public.validate_secret_key_constraints() IS 
    'Ensures secret keys have required fields: application_id, quota, and retention settings';


-- ============================================================================
-- 8. Grant appropriate permissions (adjust role names as needed)
-- ============================================================================

-- Allow application to call the function
GRANT EXECUTE ON FUNCTION public.get_application_with_keys(UUID) TO vaultless;
GRANT EXECUTE ON FUNCTION public.refresh_applications_with_keys_view() TO vaultless;

-- Allow application to read the materialized view
GRANT SELECT ON public.mv_applications_with_keys TO vaultless;