-- ============================================================================
-- Migration: Add client_count to mv_applications_with_usage
-- ============================================================================
-- Adds a count of clients registered under each application to the
-- materialized view for dashboard display purposes.
-- ============================================================================

-- ============================================================================
-- 1. Drop existing materialized view and dependencies
-- ============================================================================
DROP MATERIALIZED VIEW IF EXISTS mv_applications_with_usage CASCADE;

-- ============================================================================
-- 2. Recreate materialized view with client_count
-- ============================================================================
CREATE MATERIALIZED VIEW public.mv_applications_with_usage AS
SELECT
    -- ========================================================================
    -- APPLICATION METADATA
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
    a.internal_notes,
    a.app_meta,

    -- ========================================================================
    -- SECRET KEY INFO
    -- ========================================================================
    sk.id AS secret_key_id,
    sk.tier,
    sk.monthly_message_quota,
    sk.rate_limit_per_minute,
    sk.message_retention_seconds,

    -- ========================================================================
    -- PUBLISHABLE KEYS (using LATERAL for efficiency)
    -- ========================================================================
    COALESCE(pk_data.count, 0) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,

    -- ========================================================================
    -- WEBHOOKS (using LATERAL for efficiency)
    -- ========================================================================
    COALESCE(webhook_data.count, 0) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,

    -- ========================================================================
    -- CLIENT COUNT (new field)
    -- ========================================================================
    COALESCE(client_data.count, 0) AS client_count,

    -- ========================================================================
    -- CURRENT MONTH USAGE
    -- ========================================================================
    COALESCE(current_month.total_messages_sent, 0) AS current_month_messages_sent,
    COALESCE(current_month.total_messages_received, 0) AS current_month_messages_received,
    COALESCE(current_month.total_proofs_verified, 0) AS current_month_proofs_verified,
    COALESCE(current_month.total_bytes_stored, 0) AS current_month_bytes_stored,
    COALESCE(current_month.total_bytes_sent, 0) AS current_month_bytes_sent,
    COALESCE(current_month.total_bytes_received, 0) AS current_month_bytes_received,
    COALESCE(current_month.total_rate_limit_hits, 0) AS current_month_rate_limit_hits,
    COALESCE(current_month.total_estimated_cost_cents, 0) AS current_month_cost_cents,

    -- Quota percentage
    CASE
        WHEN sk.monthly_message_quota > 0 THEN
            (COALESCE(current_month.total_messages_sent, 0)::float / sk.monthly_message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,

    -- ========================================================================
    -- LIFETIME USAGE
    -- ========================================================================
    COALESCE(lifetime.total_messages_sent, 0) AS lifetime_messages_sent,
    COALESCE(lifetime.total_messages_received, 0) AS lifetime_messages_received,
    COALESCE(lifetime.total_proofs_verified, 0) AS lifetime_proofs_verified,
    COALESCE(lifetime.total_bytes_stored, 0) AS lifetime_bytes_stored,
    COALESCE(lifetime.total_bytes_sent, 0) AS lifetime_bytes_sent,
    COALESCE(lifetime.total_bytes_received, 0) AS lifetime_bytes_received,
    COALESCE(lifetime.total_rate_limit_hits, 0) AS lifetime_rate_limit_hits,
    COALESCE(lifetime.total_estimated_cost_cents, 0) AS lifetime_cost_cents,

    -- ========================================================================
    -- TREND USAGE (7d, 30d)
    -- ========================================================================
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
                'expires_at', pk.expires_at,
                'last_used_at', pk.last_used_at
            ) ORDER BY pk.created_at DESC
        ) AS keys_json
    FROM public.api_keys pk
    WHERE pk.application_id = a.id
      AND pk.key_type = 'publishable'
      AND pk.is_active = true
) pk_data ON true

-- ============================================================================
-- LATERAL: Webhooks
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
-- LATERAL: Client Count (new)
-- ============================================================================
LEFT JOIN LATERAL (
    SELECT COUNT(c.id) AS count
    FROM public.clients c
    WHERE c.application_id = a.id
      AND c.is_active = true
) client_data ON true

-- ============================================================================
-- LATERAL: Current Month Usage
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
    WHERE application_id = sk.id
      AND day >= date_trunc('month', NOW())
) current_month ON true

-- ============================================================================
-- LATERAL: Lifetime Usage
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
    WHERE application_id = sk.id
) lifetime ON true

-- ============================================================================
-- LATERAL: Last 7 Days
-- ============================================================================
LEFT JOIN LATERAL (
    SELECT
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_bytes_sent) AS total_bytes_sent,
        SUM(total_bytes_received) AS total_bytes_received,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE application_id = sk.id
      AND day >= NOW() - INTERVAL '7 days'
) last_7d ON true

-- ============================================================================
-- LATERAL: Last 30 Days
-- ============================================================================
LEFT JOIN LATERAL (
    SELECT
        SUM(total_messages_sent) AS total_messages_sent,
        SUM(total_bytes_sent) AS total_bytes_sent,
        SUM(total_bytes_received) AS total_bytes_received,
        SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM usage_metrics_daily
    WHERE application_id = sk.id
      AND day >= NOW() - INTERVAL '30 days'
) last_30d ON true;

-- ============================================================================
-- 3. Recreate indexes for the materialized view
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

-- New index for client count queries
CREATE INDEX idx_mv_app_usage_client_count
    ON public.mv_applications_with_usage (user_id, client_count DESC);

-- ============================================================================
-- 4. Update comments
-- ============================================================================
COMMENT ON MATERIALIZED VIEW public.mv_applications_with_usage IS
    'Complete materialized view with applications, keys, webhooks, client counts, and usage metrics. Refresh after app/key/webhook/client changes.';

-- ============================================================================
-- 5. Grant permissions
-- ============================================================================
GRANT SELECT ON public.mv_applications_with_usage TO vaultless;

-- ============================================================================
-- 6. Refresh the view
-- ============================================================================
REFRESH MATERIALIZED VIEW public.mv_applications_with_usage;
