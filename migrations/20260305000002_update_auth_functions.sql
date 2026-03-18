-- ============================================================================
-- Migration: Update auth config resolution functions
-- Description: Resolve subscriptions via developer_id instead of applications.subscription_id
-- ============================================================================

BEGIN;

CREATE OR REPLACE FUNCTION public.fetch_auth_config_by_publishable_key(
    pk_plaintext text
)
RETURNS TABLE (
    -- Application Fields
    app_id uuid,
    app_user_id uuid,
    app_name character varying,
    app_description text,
    app_is_active boolean,
    app_max_ttl_seconds integer,
    app_is_key_rotation_forced boolean,
    app_meta jsonb,

    -- Secret Key Fields (Internal Audit/Prefix)
    sk_id uuid,
    sk_key_prefix character varying,

    -- Shared Subscription Fields (The Bundle source)
    sub_tier subscription_tier,
    sub_monthly_message_quota BIGINT,
    sub_message_retention_seconds BIGINT,
    sub_rate_limit_per_minute integer
)
LANGUAGE sql
STABLE
AS $$
    -- Step 1: Find the Application linked to this Publishable Key
    WITH app_lookup AS (
        SELECT application_id
        FROM public.api_keys
        WHERE publishable_key_plaintext = pk_plaintext
          AND key_type = 'publishable'::key_type
          AND is_active = TRUE
        LIMIT 1
    )
    -- Step 2: Join Application -> Subscription AND Application -> Active Secret Key
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.applications a
    JOIN app_lookup al ON a.id = al.application_id
    -- Link via application.developer_id
    JOIN public.developer_subscriptions s ON a.developer_id = s.developer_id AND s.is_active = TRUE
    -- Optional: Get the current active secret key if one exists
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
    -- Application Fields
    app_id uuid,
    app_user_id uuid,
    app_name character varying,
    app_description text,
    app_is_active boolean,
    app_max_ttl_seconds integer,
    app_is_key_rotation_forced boolean,
    app_meta jsonb,

    -- Secret Key Fields
    sk_id uuid,
    sk_key_prefix character varying,

    -- Shared Subscription Fields
    sub_tier subscription_tier,
    sub_monthly_message_quota BIGINT,
    sub_message_retention_seconds BIGINT,
    sub_rate_limit_per_minute integer
)
LANGUAGE sql
STABLE
AS $$
    -- Direct chain: API Key -> Application -> Subscription (via developer_id)
    SELECT
        a.id, a.developer_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM public.api_keys sk
    INNER JOIN public.applications a ON sk.application_id = a.id
    -- Link via application.developer_id
    INNER JOIN public.developer_subscriptions s ON a.developer_id = s.developer_id AND s.is_active = TRUE
    WHERE sk.key_hash = sk_hash_hex
      AND sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
      AND a.is_active = TRUE
    LIMIT 1;
$$;

COMMIT;
