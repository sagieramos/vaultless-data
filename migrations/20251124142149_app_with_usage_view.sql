-- ============================================================================
-- Migration: Unified Applications with Shared Subscriptions, Clients & Full Metrics
-- ============================================================================
BEGIN;

-- 1. Pre-requisites & Indexing
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_one_secret_per_app
    ON public.api_keys (application_id)
    WHERE key_type = 'secret' AND is_active = true;

-- 2. Clean Slate for the View
DROP MATERIALIZED VIEW IF EXISTS public.mv_applications_with_usage CASCADE;

-- 3. The Unified View
CREATE MATERIALIZED VIEW public.mv_applications_with_usage AS
SELECT 
    -- APPLICATION METADATA
    a.id AS application_id,
    a.developer_id,
    a.name,
    a.description,
    a.is_active,
    a.created_at,
    a.updated_at,
    a.app_meta,
    
    -- SHARED SUBSCRIPTION POOL
    s.id AS subscription_id,
    s.tier,
    s.monthly_message_quota,
    s.rate_limit_per_minute,
    s.message_retention_seconds,
    
    -- SECRET KEY IDENTITY
    sk.id AS secret_key_id,
    sk.key_prefix AS secret_key_prefix,
    
    -- PUBLISHABLE KEYS (LATERAL)
    COALESCE(pk_data.count, 0) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,
    
    -- WEBHOOKS (LATERAL)
    COALESCE(webhook_data.count, 0) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,

    -- CLIENT COUNT (LATERAL)
    COALESCE(client_data.count, 0) AS client_count,
    
    -- CURRENT MONTH USAGE (Full Metrics Suite)
    COALESCE(current_month.total_messages_sent, 0) AS current_month_messages_sent,
    COALESCE(current_month.total_messages_received, 0) AS current_month_messages_received,
    COALESCE(current_month.total_proofs_verified, 0) AS current_month_proofs_verified,
    COALESCE(current_month.total_bytes_stored, 0) AS current_month_bytes_stored,
    COALESCE(current_month.total_bytes_sent, 0) AS current_month_bytes_sent,
    COALESCE(current_month.total_bytes_received, 0) AS current_month_bytes_received,
    COALESCE(current_month.total_rate_limit_hits, 0) AS current_month_rate_limit_hits,
    COALESCE(current_month.total_estimated_cost_cents, 0) AS current_month_cost_cents,
    
    -- Quota percentage (Calculated against the shared Subscription pool)
    CASE 
        WHEN s.monthly_message_quota > 0 THEN 
            (COALESCE(current_month.total_messages_sent, 0)::float / s.monthly_message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,
    
    -- LIFETIME USAGE (Aggregated by Application ID)
    COALESCE(lifetime.total_messages_sent, 0) AS lifetime_messages_sent,
    COALESCE(lifetime.total_estimated_cost_cents, 0) AS lifetime_cost_cents

FROM public.applications a
LEFT JOIN public.developer_subscriptions s ON a.subscription_id = s.id
LEFT JOIN public.api_keys sk ON (sk.application_id = a.id AND sk.key_type = 'secret' AND sk.is_active = true)

-- LATERAL: Publishable Keys
LEFT JOIN LATERAL (
    SELECT COUNT(pk.id) AS count,
           jsonb_agg(jsonb_build_object('id', pk.id, 'key_prefix', pk.key_prefix, 'is_active', pk.is_active) 
           ORDER BY pk.created_at DESC) AS keys_json
    FROM public.api_keys pk
    WHERE pk.application_id = a.id AND pk.key_type = 'publishable' AND pk.is_active = true
) pk_data ON true

-- LATERAL: Client Count
LEFT JOIN LATERAL (
    SELECT COUNT(c.id) AS count
    FROM public.clients c
    WHERE c.application_id = a.id AND c.is_active = true
) client_data ON true

-- LATERAL: Monthly Metrics
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
    WHERE application_id = a.id  -- Linked to App ID for rotation safety
      AND day >= date_trunc('month', NOW())
) current_month ON true

-- LATERAL: Lifetime Summary
LEFT JOIN LATERAL (
    SELECT SUM(total_messages_sent) AS total_messages_sent,
           SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE application_id = a.id
) lifetime ON true

-- LATERAL: Webhooks
LEFT JOIN LATERAL (
    SELECT COUNT(w.id) AS count,
           jsonb_agg(jsonb_build_object('id', w.id, 'url', w.url, 'event_type', w.event_type)) AS webhooks_json
    FROM public.webhooks w
    WHERE w.application_id = a.id AND w.is_active = true
) webhook_data ON true;

-- 4. Recreate Indexes
CREATE UNIQUE INDEX idx_mv_app_usage_app_id ON public.mv_applications_with_usage (application_id);
CREATE INDEX idx_mv_app_usage_developer_id ON public.mv_applications_with_usage (developer_id);
CREATE INDEX idx_mv_app_usage_client_count ON public.mv_applications_with_usage (developer_id, client_count DESC);
CREATE INDEX idx_mv_app_usage_quota_warning ON public.mv_applications_with_usage (developer_id, quota_usage_percentage DESC) WHERE quota_usage_percentage >= 80;

-- 6. Refresh and Permissions
GRANT SELECT ON public.mv_applications_with_usage TO vaultless;
REFRESH MATERIALIZED VIEW public.mv_applications_with_usage;

COMMIT;