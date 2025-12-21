-- ============================================================================
-- Updated Auth Config Functions (Subscription-Aware)
-- ============================================================================

-- 1. Fetch by Publishable Key (Client-side lookup)
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
    app_app_meta jsonb,

    -- Secret Key Fields (Internal Audit/Prefix)
    sk_id uuid,
    sk_key_prefix character varying,

    -- Subscription Fields (The new "Source of Truth" for Quotas)
    sub_tier subscription_tier,
    sub_monthly_message_quota BIGINT,
    sub_message_retention_seconds BIGINT,
    sub_rate_limit_per_minute integer
)
LANGUAGE sql
AS $$
    -- Step 1: Resolve the Application via the Publishable Key
    WITH app_lookup AS (
        SELECT application_id
        FROM api_keys
        WHERE publishable_key_plaintext = pk_plaintext
          AND key_type = 'publishable'::key_type
          AND is_active = TRUE
        LIMIT 1
    )
    -- Step 2: Join App + Active Secret Key + Active Subscription
    SELECT
        a.id, a.user_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM applications a
    JOIN app_lookup al ON a.id = al.application_id
    JOIN subscriptions s ON a.id = s.application_id
    LEFT JOIN api_keys sk ON a.id = sk.application_id 
        AND sk.key_type = 'secret'::key_type 
        AND sk.is_active = TRUE
    WHERE a.is_active = TRUE
      AND s.is_active = TRUE
    LIMIT 1;
$$;

-- 2. Fetch by Secret Hash (Server-side validation)
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
    app_app_meta jsonb,

    -- Secret Key Fields
    sk_id uuid,
    sk_key_prefix character varying,

    -- Subscription Fields
    sub_tier subscription_tier,
    sub_monthly_message_quota BIGINT,
    sub_message_retention_seconds BIGINT,
    sub_rate_limit_per_minute integer
)
LANGUAGE sql
AS $$
    -- Join Secret Key (SK) -> Application (A) -> Subscription (S)
    SELECT
        a.id, a.user_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.app_meta,
        sk.id, sk.key_prefix,
        s.tier, s.monthly_message_quota, s.message_retention_seconds,
        s.rate_limit_per_minute
    FROM api_keys sk
    JOIN applications a ON sk.application_id = a.id
    JOIN subscriptions s ON a.id = s.application_id
    WHERE sk.key_hash = sk_hash_hex
      AND sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
      AND a.is_active = TRUE
      AND s.is_active = TRUE
    LIMIT 1;
$$;