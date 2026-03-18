-- ============================================================================
-- Migration: Rename subscription columns and add new fields
-- ============================================================================

BEGIN;

-- 1. Rename columns
ALTER TABLE public.developer_subscriptions RENAME COLUMN monthly_message_quota TO message_quota;
ALTER TABLE public.developer_subscriptions RENAME COLUMN monthly_bandwidth_quota TO bandwidth_quota;
ALTER TABLE public.developer_subscriptions RENAME COLUMN current_period_start TO period_start;
ALTER TABLE public.developer_subscriptions RENAME COLUMN current_period_end TO period_end;

-- 2. Add new columns
ALTER TABLE public.developer_subscriptions
    ADD COLUMN bandwidth_rate_limit_bytes bigint NOT NULL DEFAULT 10485760,
    ADD COLUMN proof_enabled boolean NOT NULL DEFAULT false;

-- 3. Drop auth functions (return type changed)
DROP FUNCTION IF EXISTS public.fetch_auth_config_by_publishable_key(text);
DROP FUNCTION IF EXISTS public.fetch_auth_config_by_secret_hash(text);

-- 4. Recreate auth functions
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
    sub_message_quota BIGINT,
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
        s.tier, s.message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.applications a
    JOIN app_lookup al ON a.id = al.application_id
    LEFT JOIN LATERAL (
        SELECT ds.*
        FROM public.developer_subscriptions ds
        WHERE ds.application_id = a.id
          AND ds.is_active = TRUE
          AND (ds.period_end IS NULL OR ds.period_end > NOW())
        ORDER BY ds.created_at DESC
        LIMIT 1
    ) s ON true
    LEFT JOIN public.api_keys sk ON a.id = sk.application_id
        AND sk.key_type = 'secret'::key_type
        AND sk.is_active = TRUE
    WHERE a.is_active = TRUE
    LIMIT 1;
$$;

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
    sub_message_quota BIGINT,
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
        s.tier, s.message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.api_keys sk
    INNER JOIN public.applications a ON sk.application_id = a.id
    LEFT JOIN LATERAL (
        SELECT ds.*
        FROM public.developer_subscriptions ds
        WHERE ds.application_id = a.id
          AND ds.is_active = TRUE
          AND (ds.period_end IS NULL OR ds.period_end > NOW())
        ORDER BY ds.created_at DESC
        LIMIT 1
    ) s ON true
    WHERE sk.key_hash = sk_hash_hex
      AND sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
      AND a.is_active = TRUE
    LIMIT 1;
$$;

-- 4. Update create_application
CREATE OR REPLACE FUNCTION public.create_application(
    p_user_id uuid,
    p_name text,
    p_description text DEFAULT NULL::text,
    p_max_ttl_seconds integer DEFAULT 604800,
    p_is_key_rotation_forced boolean DEFAULT false,
    p_secret_key_hash text DEFAULT NULL::text,
    p_secret_key_prefix text DEFAULT NULL::text,
    p_publishable_key_plaintext text DEFAULT NULL::text,
    p_publishable_key_prefix text DEFAULT NULL::text
)
RETURNS TABLE(
    application_id uuid,
    user_id uuid,
    subscription_id uuid,
    name text,
    description text,
    is_active boolean,
    created_at timestamp with time zone,
    updated_at timestamp with time zone,
    max_ttl_seconds bigint,
    is_key_rotation_forced boolean,
    deletion_requested_at timestamp with time zone,
    internal_notes text,
    app_meta jsonb,
    secret_key_prefix text,
    publishable_key_plaintext text
)
LANGUAGE plpgsql
AS $function$
DECLARE
    v_app_id UUID;
    v_app_row applications%ROWTYPE;
    v_subscription_id UUID;
BEGIN
    IF LENGTH(TRIM(p_name)) = 0 THEN
        RAISE EXCEPTION 'Application name cannot be empty';
    END IF;

    IF p_max_ttl_seconds IS NOT NULL AND p_max_ttl_seconds <= 0 THEN
        RAISE EXCEPTION 'Max TTL seconds must be positive';
    END IF;

    -- Create application
    v_app_id := gen_random_uuid();
    INSERT INTO applications (
        id, developer_id, name, description,
        max_ttl_seconds, is_key_rotation_forced
    )
    VALUES (
        v_app_id, p_user_id, p_name, p_description,
        COALESCE(p_max_ttl_seconds, 604800),
        COALESCE(p_is_key_rotation_forced, FALSE)
    )
    RETURNING * INTO v_app_row;

    -- Create free subscription (expires in 30 days)
    INSERT INTO developer_subscriptions (
        developer_id, application_id, tier,
        period_start, period_end
    )
    VALUES (
        p_user_id, v_app_id, 'free'::subscription_tier,
        NOW(), NOW() + INTERVAL '30 days'
    )
    RETURNING id INTO v_subscription_id;

    -- Create secret key
    INSERT INTO api_keys (id, user_id, key_hash, key_prefix, description, is_active, created_at, updated_at, application_id, key_type)
    VALUES (gen_random_uuid(), p_user_id, p_secret_key_hash, p_secret_key_prefix, 'Secret key for ' || p_name, TRUE, NOW(), NOW(), v_app_id, 'secret'::key_type);

    -- Create publishable key
    INSERT INTO api_keys (id, user_id, key_prefix, description, is_active, created_at, updated_at, application_id, key_type, publishable_key_plaintext)
    VALUES (gen_random_uuid(), p_user_id, p_publishable_key_prefix, 'Publishable key for ' || p_name, TRUE, NOW(), NOW(), v_app_id, 'publishable'::key_type, p_publishable_key_plaintext);

    RETURN QUERY
    SELECT
        v_app_row.id,
        v_app_row.developer_id,
        v_subscription_id,
        v_app_row.name::TEXT,
        v_app_row.description::TEXT,
        v_app_row.is_active,
        v_app_row.created_at,
        v_app_row.updated_at,
        v_app_row.max_ttl_seconds::BIGINT,
        v_app_row.is_key_rotation_forced,
        v_app_row.deletion_requested_at,
        v_app_row.internal_notes::TEXT,
        v_app_row.app_meta,
        p_secret_key_prefix::TEXT,
        p_publishable_key_plaintext::TEXT;
END;
$function$;

-- 5. Update update_application
CREATE OR REPLACE FUNCTION public.update_application(
    p_app_id uuid,
    p_user_id uuid,
    p_name text DEFAULT NULL::text,
    p_description text DEFAULT NULL::text,
    p_is_active boolean DEFAULT NULL::boolean,
    p_max_ttl_seconds integer DEFAULT NULL::integer,
    p_is_key_rotation_forced boolean DEFAULT NULL::boolean,
    p_internal_notes text DEFAULT NULL::text,
    p_integrity_patch jsonb DEFAULT NULL::jsonb,
    p_webhooks jsonb DEFAULT NULL::jsonb
)
RETURNS TABLE(
    application_id uuid,
    developer_id uuid,
    subscription_id uuid,
    name text,
    description text,
    is_active boolean,
    created_at timestamp with time zone,
    updated_at timestamp with time zone,
    max_ttl_seconds integer,
    is_key_rotation_forced boolean,
    deletion_requested_at timestamp with time zone,
    internal_notes text,
    app_meta jsonb
)
LANGUAGE plpgsql
AS $function$
DECLARE
    v_subscription_id UUID;
BEGIN
    SELECT ds.id INTO v_subscription_id
    FROM developer_subscriptions ds
    WHERE ds.application_id = p_app_id AND ds.is_active = TRUE
    LIMIT 1;

    UPDATE applications a
    SET
        name = COALESCE(p_name, a.name),
        description = COALESCE(p_description, a.description),
        is_active = COALESCE(p_is_active, a.is_active),
        max_ttl_seconds = COALESCE(p_max_ttl_seconds, a.max_ttl_seconds),
        is_key_rotation_forced = COALESCE(p_is_key_rotation_forced, a.is_key_rotation_forced),
        internal_notes = COALESCE(p_internal_notes, a.internal_notes),
        app_meta = jsonb_merge_patch(a.app_meta, COALESCE(p_integrity_patch, '{}'::jsonb)),
        updated_at = NOW()
    WHERE a.id = p_app_id AND a.developer_id = p_user_id
    RETURNING
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.created_at, a.updated_at, a.max_ttl_seconds, a.is_key_rotation_forced,
        a.deletion_requested_at, a.internal_notes, a.app_meta
    INTO STRICT
        application_id, developer_id, name, description, is_active,
        created_at, updated_at, max_ttl_seconds, is_key_rotation_forced,
        deletion_requested_at, internal_notes, app_meta;

    subscription_id := v_subscription_id;

    IF p_webhooks IS NOT NULL THEN
        PERFORM sync_webhooks(p_app_id, p_webhooks);
    END IF;

    RETURN NEXT;
END;
$function$;

-- 6. Recreate materialized views
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
    s.message_quota,
    s.bandwidth_quota,
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
        WHEN s.message_quota IS NOT NULL AND s.message_quota > 0 THEN
            (COALESCE(current_month.total_messages_sent, 0)::float / s.message_quota * 100)::numeric(5,2)
        ELSE 0
    END AS quota_usage_percentage,

    CASE
        WHEN s.bandwidth_quota IS NOT NULL AND s.bandwidth_quota > 0 THEN
            (COALESCE(current_month.total_bytes_sent + current_month.total_bytes_received, 0)::float / s.bandwidth_quota * 100)::numeric(5,2)
        ELSE 0
    END AS bandwidth_quota_usage_percentage,

    COALESCE(lifetime.total_messages_sent, 0)::BIGINT AS lifetime_messages_sent
FROM public.applications a
LEFT JOIN LATERAL (
    SELECT ds.*
    FROM public.developer_subscriptions ds
    WHERE ds.application_id = a.id
      AND ds.is_active = true
      AND (ds.period_end IS NULL OR ds.period_end > NOW())
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

CREATE UNIQUE INDEX IF NOT EXISTS idx_mv_app_usage_app_id ON public.mv_applications_with_usage (application_id);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_developer_id ON public.mv_applications_with_usage (developer_id);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_client_count ON public.mv_applications_with_usage (developer_id, client_count DESC);
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_quota_warning ON public.mv_applications_with_usage (developer_id, quota_usage_percentage DESC) WHERE quota_usage_percentage >= 80;
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_bandwidth_warning ON public.mv_applications_with_usage (developer_id, bandwidth_quota_usage_percentage DESC) WHERE bandwidth_quota_usage_percentage >= 80;
CREATE INDEX IF NOT EXISTS idx_mv_app_usage_revenue_warning ON public.mv_applications_with_usage (developer_id, current_month_revenue_cents DESC) WHERE current_month_revenue_cents > 0;

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

GRANT SELECT ON public.mv_applications_with_usage TO vaultless;
GRANT SELECT ON public.mv_attached_apps_by_plan TO vaultless;

REFRESH MATERIALIZED VIEW public.mv_applications_with_usage;
REFRESH MATERIALIZED VIEW public.mv_attached_apps_by_plan;

COMMIT;
