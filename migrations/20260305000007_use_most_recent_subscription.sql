-- ============================================================================
-- Migration: Use most recent non-expired subscription for auth lookups
-- Description: Only the most recently created, non-expired subscription
--              should be used for an application.
-- ============================================================================

BEGIN;

-- 1. Fix fetch_auth_config_by_publishable_key
CREATE OR REPLACE FUNCTION public.fetch_auth_config_by_publishable_key(
    pk_plaintext text
)
RETURNS TABLE (
    app_id uuid,
    app_user_id uuid,
    app_name character varying,
    app_description text,
    app_is_active boolean,
    app_max_ttl_seconds integer,
    app_is_key_rotation_forced boolean,
    app_meta jsonb,
    sk_id uuid,
    sk_key_prefix character varying,
    sub_tier subscription_tier,
    sub_monthly_message_quota BIGINT,
    sub_message_retention_seconds BIGINT,
    sub_rate_limit_per_minute integer
)
LANGUAGE sql
STABLE
AS $$
    WITH app_lookup AS (
        SELECT application_id
        FROM public.api_keys
        WHERE publishable_key_plaintext = pk_plaintext
          AND key_type = 'publishable'::key_type
          AND is_active = TRUE
        LIMIT 1
    )
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.applications a
    JOIN app_lookup al ON a.id = al.application_id
    LEFT JOIN LATERAL (
        SELECT ds.*
        FROM public.developer_subscriptions ds
        WHERE ds.application_id = a.id
          AND ds.is_active = TRUE
          AND (ds.current_period_end IS NULL OR ds.current_period_end > NOW())
        ORDER BY ds.created_at DESC
        LIMIT 1
    ) s ON true
    LEFT JOIN public.api_keys sk ON a.id = sk.application_id
        AND sk.key_type = 'secret'::key_type
        AND sk.is_active = TRUE
    WHERE a.is_active = TRUE
    LIMIT 1;
$$;

-- 2. Fix fetch_auth_config_by_secret_hash
CREATE OR REPLACE FUNCTION public.fetch_auth_config_by_secret_hash(
    sk_hash_hex text
)
RETURNS TABLE (
    app_id uuid,
    app_user_id uuid,
    app_name character varying,
    app_description text,
    app_is_active boolean,
    app_max_ttl_seconds integer,
    app_is_key_rotation_forced boolean,
    app_meta jsonb,
    sk_id uuid,
    sk_key_prefix character varying,
    sub_tier subscription_tier,
    sub_monthly_message_quota BIGINT,
    sub_message_retention_seconds BIGINT,
    sub_rate_limit_per_minute integer
)
LANGUAGE sql
STABLE
AS $$
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.api_keys sk
    INNER JOIN public.applications a ON sk.application_id = a.id
    LEFT JOIN LATERAL (
        SELECT ds.*
        FROM public.developer_subscriptions ds
        WHERE ds.application_id = a.id
          AND ds.is_active = TRUE
          AND (ds.current_period_end IS NULL OR ds.current_period_end > NOW())
        ORDER BY ds.created_at DESC
        LIMIT 1
    ) s ON true
    WHERE sk.key_hash = sk_hash_hex
      AND sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
      AND a.is_active = TRUE
    LIMIT 1;
$$;

-- 3. Fix materialized view
DROP MATERIALIZED VIEW IF EXISTS public.mv_attached_apps_by_plan CASCADE;
DROP MATERIALIZED VIEW IF EXISTS public.mv_applications_with_usage CASCADE;

CREATE MATERIALIZED VIEW public.mv_applications_with_usage AS
SELECT
    a.id AS application_id,
    a.developer_id,
    a.name,
    a.description,
    a.is_active,
    a.created_at,
    a.updated_at,
    a.app_meta,

    s.id AS subscription_id,
    s.tier,
    s.monthly_message_quota,
    s.monthly_bandwidth_quota,
    s.rate_limit_per_minute,
    s.message_retention_seconds,

    sk.id AS secret_key_id,
    sk.key_prefix AS secret_key_prefix,
    sk.scopes AS secret_key_scopes,
    sk.is_active AS secret_key_is_active,
    jsonb_build_object(
        'createdAt', sk.created_at,
        'expiresAt', sk.expires_at
    ) AS secret_key_timestamps,

    COALESCE(pk_data.count, 0) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,

    COALESCE(webhook_data.count, 0) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,

    COALESCE(client_data.count, 0) AS client_count,

    COALESCE(current_month.total_messages_sent, 0)::BIGINT AS current_month_messages_sent,
    COALESCE(current_month.total_messages_received, 0)::BIGINT AS current_month_messages_received,
    COALESCE(current_month.total_proofs_verified, 0)::BIGINT AS current_month_proofs_verified,
    COALESCE(current_month.total_bytes_stored, 0)::BIGINT AS current_month_bytes_stored,
    COALESCE(current_month.total_bytes_sent, 0)::BIGINT AS current_month_bytes_sent,
    COALESCE(current_month.total_bytes_received, 0)::BIGINT AS current_month_bytes_received,
    COALESCE(current_month.total_rate_limit_hits, 0)::BIGINT AS current_month_rate_limit_hits,

    COALESCE(revenue_data.current_month_revenue_cents, 0)::BIGINT AS current_month_revenue_cents,
    COALESCE(revenue_data.billable_clients_count, 0)::INTEGER AS billable_clients_count,

    CASE
        WHEN s.monthly_message_quota IS NOT NULL AND s.monthly_message_quota > 0 THEN
            (COALESCE(current_month.total_messages_sent, 0)::float / s.monthly_message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,

    CASE
        WHEN s.monthly_bandwidth_quota IS NOT NULL AND s.monthly_bandwidth_quota > 0 THEN
            (COALESCE(current_month.total_bytes_sent + current_month.total_bytes_received, 0)::float / s.monthly_bandwidth_quota * 100)::numeric(5,2)
        ELSE 0
    END AS bandwidth_quota_usage_percentage,

    COALESCE(lifetime.total_messages_sent, 0)::BIGINT AS lifetime_messages_sent
FROM public.applications a
LEFT JOIN LATERAL (
    SELECT ds.*
    FROM public.developer_subscriptions ds
    WHERE ds.application_id = a.id
      AND ds.is_active = true
      AND (ds.current_period_end IS NULL OR ds.current_period_end > NOW())
    ORDER BY ds.created_at DESC
    LIMIT 1
) s ON true
LEFT JOIN public.api_keys sk ON (sk.application_id = a.id AND sk.key_type = 'secret'::public.key_type AND sk.is_active = true)
LEFT JOIN LATERAL (
    SELECT COUNT(pk.id) AS count,
           jsonb_agg(
               jsonb_build_object(
                   'id', pk.id,
                   'keyPrefix', pk.key_prefix,
                   'publishableKeyPlaintext', pk.publishable_key_plaintext,
                   'description', pk.description,
                   'isActive', pk.is_active,
                   'createdAt', pk.created_at,
                   'expiresAt', pk.expires_at,
                   'lastUsedAt', pk.last_used_at
               )
               ORDER BY pk.created_at DESC
           ) AS keys_json
    FROM public.api_keys pk
    WHERE pk.application_id = a.id
      AND pk.key_type = 'publishable'::public.key_type
      AND pk.is_active = true
) pk_data ON true
LEFT JOIN LATERAL (
    SELECT COUNT(c.id) AS count
    FROM public.clients c
    WHERE c.application_id = a.id AND c.is_active = true
) client_data ON true
LEFT JOIN LATERAL (
    SELECT
        SUM(total_messages_sent)::BIGINT AS total_messages_sent,
        SUM(total_messages_received)::BIGINT AS total_messages_received,
        SUM(total_proofs_verified)::BIGINT AS total_proofs_verified,
        SUM(total_bytes_stored)::BIGINT AS total_bytes_stored,
        SUM(total_bytes_sent)::BIGINT AS total_bytes_sent,
        SUM(total_bytes_received)::BIGINT AS total_bytes_received,
        SUM(total_rate_limit_hits)::BIGINT AS total_rate_limit_hits
    FROM application_usage_metrics_daily umd
    WHERE umd.application_id = a.id
      AND umd.day >= date_trunc('month', NOW())
) current_month ON true
LEFT JOIN LATERAL (
    SELECT
        SUM((COALESCE(cbu.revenue_snapshot->>'total_cost_cents', '0'))::BIGINT) AS current_month_revenue_cents,
        COUNT(DISTINCT cbu.client_id) AS billable_clients_count
    FROM public.client_billing_usage cbu
    JOIN public.billing_periods bp ON cbu.billing_period_id = bp.id
    WHERE cbu.application_id = a.id
      AND bp.period_start >= date_trunc('month', NOW())
      AND bp.period_end <= NOW()
      AND bp.status != 'closed'
) revenue_data ON true
LEFT JOIN LATERAL (
    SELECT SUM(total_messages_sent) AS total_messages_sent
    FROM application_usage_metrics_daily umd
    WHERE umd.application_id = a.id
) lifetime ON true
LEFT JOIN LATERAL (
    SELECT COUNT(w.id) AS count,
           jsonb_agg(
               jsonb_build_object(
                   'id', w.id,
                   'url', w.url,
                   'eventType', w.event_type,
                   'isActive', w.is_active,
                   'createdAt', w.created_at,
                   'updatedAt', w.updated_at
               )
           ) AS webhooks_json
    FROM public.webhooks w
    WHERE w.application_id = a.id AND w.is_active = true
) webhook_data ON true;

-- Indexes
CREATE UNIQUE INDEX IF NOT EXISTS idx_mv_app_usage_app_id ON public.mv_applications_with_usage (application_id);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_developer_id ON public.mv_applications_with_usage (developer_id);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_client_count ON public.mv_applications_with_usage (developer_id, client_count DESC);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_quota_warning ON public.mv_applications_with_usage (developer_id, quota_usage_percentage DESC) WHERE quota_usage_percentage >= 80;
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_bandwidth_warning ON public.mv_applications_with_usage (developer_id, bandwidth_quota_usage_percentage DESC) WHERE bandwidth_quota_usage_percentage >= 80;
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_revenue_warning ON public.mv_applications_with_usage (developer_id, current_month_revenue_cents DESC) WHERE current_month_revenue_cents > 0;

-- Recreate mv_attached_apps_by_plan
CREATE MATERIALIZED VIEW public.mv_attached_apps_by_plan AS
SELECT
    app_plan.pricing_plan_id,
    a.application_id,
    a.developer_id,
    a.name,
    a.is_active,
    a.created_at,
    a.quota_usage_percentage,
    a.bandwidth_quota_usage_percentage,
    a.current_month_revenue_cents
FROM public.mv_applications_with_usage a
JOIN public.application_pricing_plans app_plan ON (a.application_id = app_plan.application_id);

CREATE INDEX idx_mv_attached_apps_plan_id ON public.mv_attached_apps_by_plan (pricing_plan_id);

-- Permissions
GRANT SELECT ON public.mv_applications_with_usage TO vaultless;
GRANT SELECT ON public.mv_attached_apps_by_plan TO vaultless;

-- Refresh
REFRESH MATERIALIZED VIEW public.mv_applications_with_usage;
REFRESH MATERIALIZED VIEW public.mv_attached_apps_by_plan;

COMMIT;
