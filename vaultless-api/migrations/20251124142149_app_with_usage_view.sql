-- ============================================================================
-- Migration: Complete Applications with Keys, Webhooks, and Usage
-- ============================================================================
-- SQLx migration file: migrations/{timestamp}_applications_with_usage_view.sql

-- ============================================================================
-- 1. UNIQUE CONSTRAINT: One secret key per application
-- ============================================================================
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_one_secret_per_app
    ON public.api_keys (application_id)
    WHERE key_type = 'secret' AND is_active = true;

COMMENT ON INDEX idx_api_keys_one_secret_per_app IS 
    'Ensures exactly one active secret key exists per application at database level';

-- ============================================================================
-- 2. Remove old publishable key limit trigger (if exists)
-- ============================================================================
DROP TRIGGER IF EXISTS trigger_check_max_publishable_keys ON public.api_keys;

COMMENT ON TABLE public.api_keys IS 
    'API keys table: One secret key per application (enforced by partial unique index), unlimited publishable keys for rotation';

-- ============================================================================
-- 3. Drop old views if they exist
-- ============================================================================
DROP MATERIALIZED VIEW IF EXISTS mv_applications_with_keys CASCADE;
DROP MATERIALIZED VIEW IF EXISTS mv_applications_with_usage CASCADE;

-- ============================================================================
-- 4. Create complete materialized view with LATERAL joins
-- ============================================================================
CREATE MATERIALIZED VIEW public.mv_applications_with_usage AS
SELECT 
    -- APPLICATION METADATA
    a.id AS application_id,
    a.user_id,
    a.name,
    a.description,
    a.is_active,
    a.created_at,
    a.updated_at,
    a.app_meta,
    
    -- SUBSCRIPTION INFO (The new source of truth for Quotas)
    s.id AS subscription_id,
    s.tier,
    s.monthly_message_quota,
    s.rate_limit_per_minute,
    s.message_retention_seconds,
    
    -- SECRET KEY IDENTITY (Prefix only, no quota data here)
    sk.id AS secret_key_id,
    sk.key_prefix AS secret_key_prefix,
    
    -- PUBLISHABLE KEYS
    COALESCE(pk_data.count, 0) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,
    
    -- WEBHOOKS
    COALESCE(webhook_data.count, 0) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,
    
    -- CURRENT MONTH USAGE (Joined on application_id, not key_id)
    COALESCE(current_month.total_messages_sent, 0) AS current_month_messages_sent,
    COALESCE(current_month.total_rate_limit_hits, 0) AS current_month_rate_limit_hits,
    COALESCE(current_month.total_estimated_cost_cents, 0) AS current_month_cost_cents,
    
    -- Quota percentage (Calculated against subscription limit)
    CASE 
        WHEN s.monthly_message_quota > 0 THEN 
            (COALESCE(current_month.total_messages_sent, 0)::float / s.monthly_message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,
    
    -- LIFETIME USAGE
    COALESCE(lifetime.total_messages_sent, 0) AS lifetime_messages_sent,
    COALESCE(lifetime.total_estimated_cost_cents, 0) AS lifetime_cost_cents

FROM public.applications a
-- JOIN TO SUBSCRIPTIONS (One subscription per app)
JOIN public.subscriptions s ON a.id = s.application_id AND s.is_active = true
-- LEFT JOIN TO ACTIVE SECRET KEY
LEFT JOIN public.api_keys sk ON (
    sk.application_id = a.id 
    AND sk.key_type = 'secret' 
    AND sk.is_active = true
)

-- LATERAL: Publishable Keys
LEFT JOIN LATERAL (
    SELECT 
        COUNT(pk.id) AS count,
        jsonb_agg(jsonb_build_object(
            'id', pk.id, 'key_prefix', pk.key_prefix, 'is_active', pk.is_active
        ) ORDER BY pk.created_at DESC) AS keys_json
    FROM public.api_keys pk
    WHERE pk.application_id = a.id AND pk.key_type = 'publishable' AND pk.is_active = true
) pk_data ON true

-- LATERAL: Usage Metrics (Now correctly using application_id for rotation safety)
LEFT JOIN LATERAL (
    SELECT 
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_rate_limit_hits) AS total_rate_limit_hits,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE application_id = a.id  -- Fixed: Logic now key-agnostic
      AND day >= date_trunc('month', NOW())
) current_month ON true

LEFT JOIN LATERAL (
    SELECT 
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE application_id = a.id
) lifetime ON true

-- LATERAL: Webhooks
LEFT JOIN LATERAL (
    SELECT 
        COUNT(w.id) AS count,
        jsonb_agg(jsonb_build_object(
            'id', w.id, 'url', w.url, 'event_type', w.event_type
        )) AS webhooks_json
    FROM public.webhooks w
    WHERE w.application_id = a.id AND w.is_active = true
) webhook_data ON true;

-- 4. Re-apply Indexes
CREATE UNIQUE INDEX idx_mv_app_usage_app_id ON public.mv_applications_with_usage (application_id);
CREATE INDEX idx_mv_app_usage_user_id ON public.mv_applications_with_usage (user_id);

-- 5. Updated User Summary Function
CREATE OR REPLACE FUNCTION public.get_user_usage_summary(p_user_id UUID)
RETURNS TABLE (
    total_apps INTEGER,
    total_monthly_messages BIGINT,
    total_monthly_cost BIGINT,
    critical_quota_apps INTEGER
) LANGUAGE sql STABLE AS $$
    SELECT 
        COUNT(*)::INTEGER,
        COALESCE(SUM(current_month_messages_sent), 0)::BIGINT,
        COALESCE(SUM(current_month_cost_cents), 0)::BIGINT,
        COUNT(*) FILTER (WHERE quota_usage_percentage >= 90)::INTEGER
    FROM mv_applications_with_usage
    WHERE user_id = p_user_id;
$$;

-- 6. New Subscription Validation Trigger
-- Since API keys no longer hold quotas, we validate the Subscription table instead.
CREATE OR REPLACE FUNCTION public.validate_subscription_constraints()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.monthly_message_quota <= 0 THEN
        RAISE EXCEPTION 'Monthly quota must be positive';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_validate_subscription
    BEFORE INSERT OR UPDATE ON public.subscriptions
    FOR EACH ROW EXECUTE FUNCTION public.validate_subscription_constraints();

-- No GROUP BY needed with LATERAL! 

-- ============================================================================
-- 5. Create indexes for the materialized view
-- ============================================================================
CREATE UNIQUE INDEX idx_mv_app_usage_app_id 
    ON public.mv_applications_with_usage (application_id);

CREATE INDEX idx_mv_app_usage_user_id 
    ON public.mv_applications_with_usage (user_id);

CREATE INDEX idx_mv_app_usage_is_active 
    ON public.mv_applications_with_usage (is_active)
    WHERE is_active = true;

CREATE INDEX idx_mv_app_usage_quota_warning
    ON public.mv_applications_with_usage (user_id, quota_usage_percentage DESC)
    WHERE quota_usage_percentage >= 80;

CREATE INDEX idx_mv_app_usage_lifetime_cost
    ON public.mv_applications_with_usage (user_id, lifetime_cost_cents DESC);

-- ============================================================================
-- 6. Create helper function for single application lookup
-- ============================================================================
CREATE OR REPLACE FUNCTION public.get_application_with_keys(p_application_id UUID)
RETURNS TABLE (
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
    app_meta JSONB,
    secret_key_id UUID,
    publishable_keys JSONB,
    webhooks JSONB
)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
AS $$
BEGIN
    RETURN QUERY
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
        a.internal_notes,
        a.app_meta,
        sk.id AS secret_key_id,
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
        ) AS publishable_keys,
        COALESCE(
            (
                SELECT jsonb_agg(
                    jsonb_build_object(
                        'id', w.id,
                        'url', w.url,
                        'event_type', w.event_type,
                        'is_active', w.is_active,
                        'created_at', w.created_at,
                        'updated_at', w.updated_at
                    )
                    ORDER BY w.created_at DESC
                )
                FROM public.webhooks w
                WHERE w.application_id = p_application_id
                  AND w.is_active = true
            ),
            '[]'::jsonb
        ) AS webhooks
    FROM public.applications a
    LEFT JOIN public.api_keys sk ON (
        sk.application_id = a.id 
        AND sk.key_type = 'secret' 
        AND sk.is_active = true
    )
    WHERE a.id = p_application_id;
END;
$$;

-- ============================================================================
-- 7. Create refresh helper function
-- ============================================================================
CREATE OR REPLACE FUNCTION public.refresh_applications_usage_view()
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
BEGIN
    REFRESH MATERIALIZED VIEW CONCURRENTLY public.mv_applications_with_usage;
END;
$$;

-- ============================================================================
-- 8. Create helper functions for quota warnings and user summaries
-- ============================================================================
CREATE OR REPLACE FUNCTION public.get_quota_warnings(
    p_user_id UUID,
    p_threshold_percentage NUMERIC DEFAULT 80
)
RETURNS TABLE (
    application_id UUID,
    application_name VARCHAR(255),
    quota_usage_percentage NUMERIC(5,2),
    current_month_messages_sent BIGINT,
    monthly_message_quota BIGINT,
    remaining_quota BIGINT
)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
AS $$
BEGIN
    RETURN QUERY
    SELECT 
        mv.application_id,
        mv.name AS application_name,
        mv.quota_usage_percentage,
        mv.current_month_messages_sent,
        mv.monthly_message_quota,
        (mv.monthly_message_quota - mv.current_month_messages_sent)::BIGINT AS remaining_quota
    FROM mv_applications_with_usage mv
    WHERE mv.user_id = p_user_id
      AND mv.is_active = true
      AND mv.quota_usage_percentage >= p_threshold_percentage
    ORDER BY mv.quota_usage_percentage DESC;
END;
$$;

CREATE OR REPLACE FUNCTION public.get_user_usage_summary(p_user_id UUID)
RETURNS TABLE (
    total_applications INTEGER,
    active_applications INTEGER,
    total_clients BIGINT,
    total_messages_sent_current_month BIGINT,
    total_messages_received_current_month BIGINT,
    total_cost_cents_current_month BIGINT,
    total_lifetime_messages BIGINT,
    total_lifetime_cost_cents BIGINT,
    apps_over_80_percent_quota INTEGER,
    apps_over_quota INTEGER
)
LANGUAGE plpgsql
STABLE
SECURITY DEFINER
AS $$
BEGIN
    RETURN QUERY
    SELECT 
        COUNT(*)::INTEGER AS total_applications,
        COUNT(*) FILTER (WHERE is_active = true)::INTEGER AS active_applications,
        COALESCE(SUM(client_count), 0) AS total_clients,
        COALESCE(SUM(current_month_messages_sent), 0) AS total_messages_sent_current_month,
        COALESCE(SUM(current_month_messages_received), 0) AS total_messages_received_current_month,
        COALESCE(SUM(current_month_cost_cents), 0) AS total_cost_cents_current_month,
        COALESCE(SUM(lifetime_messages_sent), 0) AS total_lifetime_messages,
        COALESCE(SUM(lifetime_cost_cents), 0) AS total_lifetime_cost_cents,
        COUNT(*) FILTER (WHERE quota_usage_percentage >= 80)::INTEGER AS apps_over_80_percent_quota,
        COUNT(*) FILTER (WHERE quota_usage_percentage >= 100)::INTEGER AS apps_over_quota
    FROM mv_applications_with_usage
    WHERE user_id = p_user_id;
END;
$$;


-- ============================================================================
-- 9. Add validation trigger for secret keys
-- ============================================================================
CREATE OR REPLACE FUNCTION public.validate_secret_key_constraints()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.key_type = 'secret' AND NEW.application_id IS NULL THEN
        RAISE EXCEPTION 'Secret keys must be associated with an application';
    END IF;
    
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

DROP TRIGGER IF EXISTS trigger_validate_secret_key_constraints ON public.api_keys;

CREATE TRIGGER trigger_validate_secret_key_constraints
    BEFORE INSERT OR UPDATE ON public.api_keys
    FOR EACH ROW
    WHEN (NEW.key_type = 'secret')
    EXECUTE FUNCTION public.validate_secret_key_constraints();

-- ============================================================================
-- 10. Create optimized indexes
-- ============================================================================
CREATE INDEX IF NOT EXISTS idx_api_keys_secret_lookup
    ON public.api_keys (application_id, key_type)
    WHERE key_type = 'secret' AND is_active = true;

CREATE INDEX IF NOT EXISTS idx_api_keys_publishable_lookup
    ON public.api_keys (application_id, created_at DESC)
    WHERE key_type = 'publishable' AND is_active = true;

-- ============================================================================
-- 11. Comments
-- ============================================================================
COMMENT ON MATERIALIZED VIEW public.mv_applications_with_usage IS 
    'Complete materialized view with applications, keys, webhooks, and usage metrics. Refresh after app/key/webhook changes only (NOT usage changes - TimescaleDB handles those).';

COMMENT ON FUNCTION public.get_application_with_keys(UUID) IS 
    'Retrieves single application with keys and webhooks. Use for real-time lookups.';

COMMENT ON FUNCTION public.refresh_applications_usage_view() IS 
    'Refreshes mv_applications_with_usage. Call ONLY after app/key/webhook changes.';

COMMENT ON FUNCTION public.get_quota_warnings(UUID, NUMERIC) IS 
    'Returns applications approaching quota limits. Default threshold is 80%.';

COMMENT ON FUNCTION public.get_user_usage_summary(UUID) IS 
    'Returns aggregated usage summary across all user applications.';

-- ============================================================================
-- 12. Grant permissions
-- ============================================================================
GRANT SELECT ON public.mv_applications_with_usage TO vaultless;
GRANT EXECUTE ON FUNCTION public.get_application_with_keys(UUID) TO vaultless;
GRANT EXECUTE ON FUNCTION public.refresh_applications_usage_view() TO vaultless;
GRANT EXECUTE ON FUNCTION public.get_quota_warnings(UUID, NUMERIC) TO vaultless;
GRANT EXECUTE ON FUNCTION public.get_user_usage_summary(UUID) TO vaultless;

-- ============================================================================
-- 13. Initial refresh
-- ============================================================================
REFRESH MATERIALIZED VIEW public.mv_applications_with_usage;