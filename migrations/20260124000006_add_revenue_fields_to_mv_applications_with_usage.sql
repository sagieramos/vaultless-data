-- ============================================================================
-- Migration: Add revenue fields to existing materialized view
-- Description: Since the bandwidth quota migration was already applied, 
--              we need to update the existing materialized view to add revenue fields
-- ============================================================================

BEGIN;

-- First, let's drop the existing indexes that reference the materialized view
DROP INDEX IF EXISTS idx_mv_app_usage_revenue_warning;
DROP INDEX IF EXISTS public.idx_mv_app_usage_app_id;
DROP INDEX IF EXISTS public.idx_mv_app_usage_client_count;
DROP INDEX IF EXISTS public.idx_mv_app_usage_developer_id;
DROP INDEX IF EXISTS public.idx_mv_app_usage_quota_warning;
DROP INDEX IF EXISTS public.idx_mv_app_usage_bandwidth_warning;

-- Drop the existing materialized view
DROP MATERIALIZED VIEW IF EXISTS public.mv_applications_with_usage CASCADE;

-- Recreate the materialized view with all fields including revenue
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

    -- SHARED SUBSCRIPTION POOL (nullable)
    s.id AS subscription_id,
    s.tier,
    s.monthly_message_quota,
    s.monthly_bandwidth_quota, -- NEW COLUMN from previous migration
    s.rate_limit_per_minute,
    s.message_retention_seconds,

    -- SECRET KEY IDENTITY
    sk.id AS secret_key_id,
    sk.key_prefix AS secret_key_prefix,

    -- PUBLISHABLE KEYS (LATERAL) - FIXED CASING
    COALESCE(pk_data.count, 0) AS publishable_key_count,
    COALESCE(pk_data.keys_json, '[]'::jsonb) AS publishable_keys,

    -- WEBHOOKS (LATERAL)
    COALESCE(webhook_data.count, 0) AS webhook_count,
    COALESCE(webhook_data.webhooks_json, '[]'::jsonb) AS webhooks,

    -- CLIENT COUNT (LATERAL)
    COALESCE(client_data.count, 0) AS client_count,

    -- CURRENT MONTH USAGE (Full Metrics Suite)
    COALESCE(current_month.total_messages_sent, 0)::BIGINT AS current_month_messages_sent,
    COALESCE(current_month.total_messages_received, 0)::BIGINT AS current_month_messages_received,
    COALESCE(current_month.total_proofs_verified, 0)::BIGINT AS current_month_proofs_verified,
    COALESCE(current_month.total_bytes_stored, 0)::BIGINT AS current_month_bytes_stored,
    COALESCE(current_month.total_bytes_sent, 0)::BIGINT AS current_month_bytes_sent,
    COALESCE(current_month.total_bytes_received, 0)::BIGINT AS current_month_bytes_received,
    COALESCE(current_month.total_rate_limit_hits, 0)::BIGINT AS current_month_rate_limit_hits,
    COALESCE(current_month.total_estimated_cost_cents, 0)::BIGINT AS current_month_cost_cents,

    -- NEW: REVENUE INFORMATION
    COALESCE(revenue_data.current_month_revenue_cents, 0)::BIGINT AS current_month_revenue_cents,
    COALESCE(revenue_data.billable_clients_count, 0)::INTEGER AS billable_clients_count,

    -- Message Quota percentage (safe for null subscription)
    CASE
        WHEN s.monthly_message_quota IS NOT NULL AND s.monthly_message_quota > 0 THEN
            (COALESCE(current_month.total_messages_sent, 0)::float / s.monthly_message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,

    -- Bandwidth Quota percentage (NEW CALCULATION from previous migration)
    CASE
        WHEN s.monthly_bandwidth_quota IS NOT NULL AND s.monthly_bandwidth_quota > 0 THEN
            (COALESCE(current_month.total_bytes_sent + current_month.total_bytes_received, 0)::float / s.monthly_bandwidth_quota * 100)::numeric(5,2)
        ELSE 0
    END AS bandwidth_quota_usage_percentage,

    -- LIFETIME USAGE (Aggregated by Application ID)
    COALESCE(lifetime.total_messages_sent, 0)::BIGINT AS lifetime_messages_sent,
    COALESCE(lifetime.total_estimated_cost_cents, 0)::BIGINT AS lifetime_cost_cents

FROM public.applications a
LEFT JOIN public.developer_subscriptions s ON a.subscription_id = s.id
LEFT JOIN public.api_keys sk ON (sk.application_id = a.id AND sk.key_type = 'secret'::public.key_type AND sk.is_active = true)

-- LATERAL: Publishable Keys (camelCase matching Rust DTO)
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

-- LATERAL: Client Count
LEFT JOIN LATERAL (
    SELECT COUNT(c.id) AS count
    FROM public.clients c
    WHERE c.application_id = a.id AND c.is_active = true
) client_data ON true

-- LATERAL: Monthly Metrics (using daily rollup view)
LEFT JOIN LATERAL (
    SELECT
        SUM(total_messages_sent)::BIGINT AS total_messages_sent,
        SUM(total_messages_received)::BIGINT AS total_messages_received,
        SUM(total_proofs_verified)::BIGINT AS total_proofs_verified,
        SUM(total_bytes_stored)::BIGINT AS total_bytes_stored,
        SUM(total_bytes_sent)::BIGINT AS total_bytes_sent,
        SUM(total_bytes_received)::BIGINT AS total_bytes_received,
        SUM(total_rate_limit_hits)::BIGINT AS total_rate_limit_hits,
        SUM(total_estimated_cost_cents)::BIGINT AS total_estimated_cost_cents
    FROM _timescaledb_internal._materialized_hypertable_3 umd  -- daily view
    WHERE umd.application_id = a.id
      AND umd.day >= date_trunc('month', NOW())
) current_month ON true

-- NEW: LATERAL: Revenue Data
LEFT JOIN LATERAL (
    SELECT
        SUM(
            (COALESCE(cbu.revenue_snapshot->>'total_cost_cents', '0'))::BIGINT
        ) AS current_month_revenue_cents,
        COUNT(DISTINCT cbu.client_id) AS billable_clients_count
    FROM public.client_billing_usage cbu
    JOIN public.billing_periods bp ON cbu.billing_period_id = bp.id
    WHERE cbu.application_id = a.id
      AND bp.period_start >= date_trunc('month', NOW())
      AND bp.period_end <= NOW()
      AND bp.status != 'closed'  -- Only include open periods
) revenue_data ON true

-- LATERAL: Lifetime Summary
LEFT JOIN LATERAL (
    SELECT SUM(total_messages_sent) AS total_messages_sent,
           SUM(total_estimated_cost_cents) AS total_estimated_cost_cents
    FROM _timescaledb_internal._materialized_hypertable_3 umd
    WHERE umd.application_id = a.id
) lifetime ON true

-- LATERAL: Webhooks (camelCase)
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

-- Recreate indexes for performance
CREATE UNIQUE INDEX IF NOT EXISTS idx_mv_app_usage_app_id ON public.mv_applications_with_usage (application_id);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_developer_id ON public.mv_applications_with_usage (developer_id);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_client_count ON public.mv_applications_with_usage (developer_id, client_count DESC);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_quota_warning ON public.mv_applications_with_usage (developer_id, quota_usage_percentage DESC) WHERE quota_usage_percentage >= 80;
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_bandwidth_warning ON public.mv_applications_with_usage (developer_id, bandwidth_quota_usage_percentage DESC) WHERE bandwidth_quota_usage_percentage >= 80;
-- NEW: Index for revenue-based queries
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_revenue_warning ON public.mv_applications_with_usage (developer_id, current_month_revenue_cents DESC) WHERE current_month_revenue_cents > 0;

-- Permissions
GRANT SELECT ON public.mv_applications_with_usage TO vaultless;

-- Refresh to populate
REFRESH MATERIALIZED VIEW public.mv_applications_with_usage;

COMMIT;

-- migrate:down
-- DROP MATERIALIZED VIEW IF EXISTS public.mv_applications_with_usage;