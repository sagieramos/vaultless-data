-- ============================================================================
-- MATERIALIZED VIEW: APPLICATIONS WITH KEYS AND USAGE
-- ============================================================================
-- Combines application metadata, keys, and leverages existing TimescaleDB 
-- continuous aggregates (usage_metrics_daily, usage_metrics_weekly)
-- 
-- Refresh this view ONLY after application/key changes, NOT after usage changes
-- (TimescaleDB continuous aggregates handle usage auto-refresh)

CREATE MATERIALIZED VIEW IF NOT EXISTS public.mv_applications_with_usage AS
SELECT 
    -- ========================================================================
    -- APPLICATION METADATA (single row per application)
    -- ========================================================================
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
    
    -- ========================================================================
    -- SECRET KEY (1:1 relationship - no aggregation needed)
    -- ========================================================================
    sk.id AS secret_key_id,
    sk.tier,
    sk.monthly_message_quota,
    sk.rate_limit_per_minute,
    sk.message_retention_seconds,
    
    -- ========================================================================
    -- PUBLISHABLE KEYS (1:many - use LATERAL to avoid GROUP BY)
    -- ========================================================================
    COALESCE(pk_data.count, 0) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,
    
    -- ========================================================================
    -- WEBHOOKS (1:many - use LATERAL to avoid GROUP BY)
    -- ========================================================================
    COALESCE(webhook_data.count, 0) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,
    
    -- ========================================================================
    -- USAGE METRICS (already in LATERAL - no change needed)
    -- ========================================================================
    COALESCE(current_month.total_messages_sent, 0) AS current_month_messages_sent,
    COALESCE(current_month.total_messages_received, 0) AS current_month_messages_received,
    COALESCE(current_month.total_proofs_verified, 0) AS current_month_proofs_verified,
    COALESCE(current_month.total_bytes_stored, 0) AS current_month_bytes_stored,
    COALESCE(current_month.total_bytes_sent, 0) AS current_month_bytes_sent,
    COALESCE(current_month.total_bytes_received, 0) AS current_month_bytes_received,
    COALESCE(current_month.total_rate_limit_hits, 0) AS current_month_rate_limit_hits,
    COALESCE(current_month.total_estimated_cost_cents, 0) AS current_month_cost_cents,
    
    CASE 
        WHEN sk.monthly_message_quota > 0 THEN 
            (COALESCE(current_month.total_messages_sent, 0)::float / sk.monthly_message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,
    
    -- Lifetime usage
    COALESCE(lifetime.total_messages_sent, 0) AS lifetime_messages_sent,
    COALESCE(lifetime.total_messages_received, 0) AS lifetime_messages_received,
    COALESCE(lifetime.total_proofs_verified, 0) AS lifetime_proofs_verified,
    COALESCE(lifetime.total_bytes_stored, 0) AS lifetime_bytes_stored,
    COALESCE(lifetime.total_bytes_sent, 0) AS lifetime_bytes_sent,
    COALESCE(lifetime.total_bytes_received, 0) AS lifetime_bytes_received,
    COALESCE(lifetime.total_rate_limit_hits, 0) AS lifetime_rate_limit_hits,
    COALESCE(lifetime.total_estimated_cost_cents, 0) AS lifetime_cost_cents,
    
    -- Trends
    COALESCE(last_7d.total_messages_sent, 0) AS last_7d_messages_sent,
    COALESCE(last_7d.total_bytes_sent, 0) AS last_7d_bytes_sent,
    COALESCE(last_7d.total_bytes_received, 0) AS last_7d_bytes_received,
    COALESCE(last_7d.total_estimated_cost_cents, 0) AS last_7d_cost_cents,
    
    COALESCE(last_30d.total_messages_sent, 0) AS last_30d_messages_sent,
    COALESCE(last_30d.total_bytes_sent, 0) AS last_30d_bytes_sent,
    COALESCE(last_30d.total_bytes_received, 0) AS last_30d_bytes_received,
    COALESCE(last_30d.total_estimated_cost_cents, 0) AS last_30d_cost_cents

FROM public.applications a

-- Secret key (1:1)
LEFT JOIN public.api_keys sk ON (
    sk.application_id = a.id 
    AND sk.key_type = 'secret' 
    AND sk.is_active = true
)

-- ============================================================================
-- LATERAL: Publishable Keys (avoids GROUP BY explosion)
-- ============================================================================
LEFT JOIN LATERAL (
    SELECT 
        COUNT(pk.id) AS count,
        jsonb_agg(
            jsonb_build_object(
                'id', pk.id,
                'key_prefix', pk.key_prefix,
                'publishable_key_plaintext', pk.publishable_key_plaintext,
                'description', pk.description,
                'is_active', pk.is_active,
                'created_at', pk.created_at,
                'expires_at', pk.expires_at
            ) ORDER BY pk.created_at DESC
        ) AS keys_json
    FROM public.api_keys pk
    WHERE pk.application_id = a.id 
      AND pk.key_type = 'publishable' 
      AND pk.is_active = true
) pk_data ON true

-- ============================================================================
-- LATERAL: Webhooks (NEW - avoids GROUP BY explosion)
-- ============================================================================
LEFT JOIN LATERAL (
    SELECT 
        COUNT(w.id) AS count,
        jsonb_agg(
            jsonb_build_object(
                'id', w.id,
                'url', w.url,
                'event_type', w.event_type,
                'is_active', w.is_active,
                'created_at', w.created_at,
                'updated_at', w.updated_at
            ) ORDER BY w.created_at DESC
        ) AS webhooks_json
    FROM public.webhooks w
    WHERE w.application_id = a.id
      AND w.is_active = true
) webhook_data ON true

-- ============================================================================
-- LATERAL: Usage Aggregates
-- ============================================================================
LEFT JOIN LATERAL (
    SELECT 
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_messages_received) AS total_messages_received,
        SUM(total_proofs_verified) AS total_proofs_verified,
        SUM(total_bytes_stored) AS total_bytes_stored,
        SUM(total_bytes_sent) AS total_bytes_sent,
        SUM(total_bytes_received) AS total_bytes_received,
        SUM(total_rate_limit_hits) AS total_rate_limit_hits,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE api_key_id = sk.id
      AND day >= date_trunc('month', NOW())
) current_month ON true

LEFT JOIN LATERAL (
    SELECT 
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_messages_received) AS total_messages_received,
        SUM(total_proofs_verified) AS total_proofs_verified,
        SUM(total_bytes_stored) AS total_bytes_stored,
        SUM(total_bytes_sent) AS total_bytes_sent,
        SUM(total_bytes_received) AS total_bytes_received,
        SUM(total_rate_limit_hits) AS total_rate_limit_hits,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE api_key_id = sk.id
) lifetime ON true

LEFT JOIN LATERAL (
    SELECT 
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_bytes_sent) AS total_bytes_sent,
        SUM(total_bytes_received) AS total_bytes_received,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE api_key_id = sk.id
      AND day >= NOW() - INTERVAL '7 days'
) last_7d ON true

LEFT JOIN LATERAL (
    SELECT 
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_bytes_sent) AS total_bytes_sent,
        SUM(total_bytes_received) AS total_bytes_received,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE api_key_id = sk.id
      AND day >= NOW() - INTERVAL '30 days'
) last_30d ON true;

-- ============================================================================
-- INDEXES for mv_applications_with_usage
-- ============================================================================

CREATE UNIQUE INDEX IF NOT EXISTS idx_mv_app_usage_app_id 
    ON public.mv_applications_with_usage (application_id);

CREATE INDEX IF NOT EXISTS idx_mv_app_usage_user_id 
    ON public.mv_applications_with_usage (user_id);

CREATE INDEX IF NOT EXISTS idx_mv_app_usage_is_active 
    ON public.mv_applications_with_usage (is_active)
    WHERE is_active = true;

-- Index for quota warning queries (apps approaching limits)
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_quota_warning
    ON public.mv_applications_with_usage (user_id, quota_usage_percentage DESC)
    WHERE quota_usage_percentage >= 80;

-- Index for cost tracking queries
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_lifetime_cost
    ON public.mv_applications_with_usage (user_id, lifetime_cost_cents DESC);

COMMENT ON MATERIALIZED VIEW public.mv_applications_with_usage IS 
    'Combined view of applications with keys and usage metrics. Leverages existing TimescaleDB continuous aggregates (usage_metrics_daily). Refresh ONLY after app/key changes, NOT usage changes.';


-- ============================================================================
-- FUNCTION: Get Single Application with Real-time Usage
-- ============================================================================
-- Use this when you need absolutely current usage data (e.g., immediately after message send)

CREATE OR REPLACE FUNCTION public.get_application_with_realtime_usage(p_application_id UUID)
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
    integrity_config JSONB,
    
    secret_key_id UUID,
    tier TEXT,
    monthly_message_quota INTEGER,
    rate_limit_per_minute INTEGER,
    message_retention_seconds INTEGER,
    
    publishable_keys JSONB,
    
    -- Real-time current month usage (from raw usage_metrics table)
    current_month_messages_sent BIGINT,
    current_month_messages_received BIGINT,
    current_month_proofs_verified BIGINT,
    current_month_bytes_stored BIGINT,
    current_month_cost_cents BIGINT,
    quota_usage_percentage NUMERIC(5,2)
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
        a.integrity_config,
        
        sk.id AS secret_key_id,
        sk.tier::TEXT,
        sk.monthly_message_quota,
        sk.rate_limit_per_minute,
        sk.message_retention_seconds,
        
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
                        'expires_at', pk.expires_at
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
        
        -- Real-time current month usage from RAW usage_metrics table (not aggregated)
        COALESCE(um.messages_sent, 0) AS current_month_messages_sent,
        COALESCE(um.messages_received, 0) AS current_month_messages_received,
        COALESCE(um.proofs_verified, 0) AS current_month_proofs_verified,
        COALESCE(um.bytes_stored, 0) AS current_month_bytes_stored,
        COALESCE(um.estimated_cost_cents, 0) AS current_month_cost_cents,
        
        CASE 
            WHEN sk.monthly_message_quota > 0 THEN 
                (COALESCE(um.messages_sent, 0)::float / sk.monthly_message_quota * 100)::numeric(5,2)
            ELSE 0
        END AS quota_usage_percentage
        
    FROM public.applications a
    
    LEFT JOIN public.api_keys sk ON (
        sk.application_id = a.id 
        AND sk.key_type = 'secret' 
        AND sk.is_active = true
    )
    
    -- Real-time aggregation from RAW usage_metrics table (bypasses continuous aggregates)
    LEFT JOIN LATERAL (
        SELECT 
            SUM(messages_sent) AS messages_sent,
            SUM(messages_received) AS messages_received,
            SUM(proofs_verified) AS proofs_verified,
            SUM(total_bytes_stored) AS bytes_stored,
            SUM(estimated_cost_cents) AS estimated_cost_cents
        FROM usage_metrics
        WHERE api_key_id = sk.id
          AND period_start >= date_trunc('month', NOW())
    ) um ON true
    
    WHERE a.id = p_application_id;
END;
$$;

COMMENT ON FUNCTION public.get_application_with_realtime_usage(UUID) IS 
    'Returns application with real-time current month usage from raw usage_metrics table. Use ONLY when you need sub-minute freshness (e.g., right after message send). For dashboards, prefer mv_applications_with_usage.';


-- ============================================================================
-- FUNCTION: Refresh Applications Usage View
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

COMMENT ON FUNCTION public.refresh_applications_usage_view() IS 
    'Refreshes mv_applications_with_usage. Call this ONLY after application/key changes (create, update, delete). DO NOT call after usage metric changes - TimescaleDB continuous aggregates handle that automatically.';


-- ============================================================================
-- HELPER FUNCTION: Get Quota Warnings
-- ============================================================================
-- Returns applications approaching their quota limits

CREATE OR REPLACE FUNCTION public.get_quota_warnings(
    p_user_id UUID,
    p_threshold_percentage NUMERIC DEFAULT 80
)
RETURNS TABLE (
    application_id UUID,
    application_name VARCHAR(255),
    quota_usage_percentage NUMERIC(5,2),
    current_month_messages_sent BIGINT,
    monthly_message_quota INTEGER,
    remaining_quota INTEGER
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
        (mv.monthly_message_quota - mv.current_month_messages_sent)::INTEGER AS remaining_quota
    FROM mv_applications_with_usage mv
    WHERE mv.user_id = p_user_id
      AND mv.is_active = true
      AND mv.quota_usage_percentage >= p_threshold_percentage
    ORDER BY mv.quota_usage_percentage DESC;
END;
$$;

COMMENT ON FUNCTION public.get_quota_warnings(UUID, NUMERIC) IS 
    'Returns applications for a user that are approaching or exceeding their quota limits. Default threshold is 80%.';


-- ============================================================================
-- HELPER FUNCTION: Get Usage Summary for User
-- ============================================================================
-- Returns aggregated usage across all user's applications

CREATE OR REPLACE FUNCTION public.get_user_usage_summary(p_user_id UUID)
RETURNS TABLE (
    total_applications INTEGER,
    active_applications INTEGER,
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

COMMENT ON FUNCTION public.get_user_usage_summary(UUID) IS 
    'Returns aggregated usage summary across all of a user''s applications. Useful for account-level dashboards.';


-- ============================================================================
-- GRANT PERMISSIONS
-- ============================================================================

GRANT SELECT ON mv_applications_with_usage TO vaultless;
GRANT EXECUTE ON FUNCTION public.get_application_with_realtime_usage(UUID) TO vaultless;
GRANT EXECUTE ON FUNCTION public.refresh_applications_usage_view() TO vaultless;
GRANT EXECUTE ON FUNCTION public.get_quota_warnings(UUID, NUMERIC) TO vaultless;
GRANT EXECUTE ON FUNCTION public.get_user_usage_summary(UUID) TO vaultless;