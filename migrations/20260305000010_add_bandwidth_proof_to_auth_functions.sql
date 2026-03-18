-- ============================================================================
-- Migration: Add bandwidth_quota, bandwidth_rate_limit_bytes, proof_enabled
--            to auth function return types
-- ============================================================================

BEGIN;

DROP FUNCTION IF EXISTS public.fetch_auth_config_by_publishable_key(text);
DROP FUNCTION IF EXISTS public.fetch_auth_config_by_secret_hash(text);

CREATE FUNCTION public.fetch_auth_config_by_publishable_key(
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
    sub_rate_limit_per_minute integer,
    sub_bandwidth_quota BIGINT,
    sub_bandwidth_rate_limit_bytes BIGINT,
    sub_proof_enabled boolean
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
        s.rate_limit_per_minute, s.bandwidth_quota, s.bandwidth_rate_limit_bytes,
        s.proof_enabled
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

CREATE FUNCTION public.fetch_auth_config_by_secret_hash(
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
    sub_rate_limit_per_minute integer,
    sub_bandwidth_quota BIGINT,
    sub_bandwidth_rate_limit_bytes BIGINT,
    sub_proof_enabled boolean
)
LANGUAGE sql
STABLE
AS $$
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute, s.bandwidth_quota, s.bandwidth_rate_limit_bytes,
        s.proof_enabled
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

COMMIT;
