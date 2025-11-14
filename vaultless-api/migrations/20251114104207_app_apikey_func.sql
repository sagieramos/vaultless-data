-- Add migration script here
CREATE OR REPLACE FUNCTION public.fetch_auth_config_by_publishable_key(
    pk_plaintext text
)
RETURNS TABLE (
    -- Application Fields (App)
    app_id uuid,
    app_user_id uuid,
    app_name character varying,
    app_description text,
    app_is_active boolean,
    app_max_ttl_seconds integer,
    app_is_key_rotation_forced boolean,
    app_integrity_config jsonb,

    -- Secret Key Fields (SK)
    sk_id uuid,
    sk_key_prefix character varying,
    sk_tier subscription_tier,
    sk_monthly_message_quota integer,
    sk_message_retention_seconds integer,
    sk_rate_limit_per_minute integer
)
LANGUAGE sql
AS $$
    -- Step 1: Find the Secret Key's Application ID using the provided Publishable Key.
    WITH app_lookup AS (
        SELECT application_id
        FROM api_keys
        WHERE publishable_key_plaintext = pk_plaintext
          AND key_type = 'publishable'::key_type
          AND is_active = TRUE
        LIMIT 1
    )
    -- Step 2: Join Application (A) and its active Secret Key (SK) using the found Application ID.
    SELECT
        a.id, a.user_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.integrity_config,
        sk.id, sk.key_prefix, sk.tier, sk.monthly_message_quota, sk.message_retention_seconds,
        sk.rate_limit_per_minute
    FROM applications a
    JOIN app_lookup al ON a.id = al.application_id
    JOIN api_keys sk ON a.id = sk.application_id
    WHERE sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
    LIMIT 1;
$$;

CREATE OR REPLACE FUNCTION public.fetch_auth_config_by_secret_hash(
    sk_hash_hex text
)
RETURNS TABLE (
    -- Application Fields (App)
    app_id uuid,
    app_user_id uuid,
    app_name character varying,
    app_description text,
    app_is_active boolean,
    app_max_ttl_seconds integer,
    app_is_key_rotation_forced boolean,
    app_integrity_config jsonb,

    -- Secret Key Fields (SK)
    sk_id uuid,
    sk_key_prefix character varying,
    sk_tier subscription_tier,
    sk_monthly_message_quota integer,
    sk_message_retention_seconds integer,
    sk_rate_limit_per_minute integer
)
LANGUAGE sql
AS $$
    -- Directly join the Secret Key (SK) with its Application (A) using the unique hash.
    SELECT
        a.id, a.user_id, a.name, a.description, a.is_active,
        a.max_ttl_seconds, a.is_key_rotation_forced, a.integrity_config,
        sk.id, sk.key_prefix, sk.tier, sk.monthly_message_quota, sk.message_retention_seconds,
        sk.rate_limit_per_minute
    FROM api_keys sk
    JOIN applications a ON sk.application_id = a.id
    WHERE sk.key_hash = sk_hash_hex
      AND sk.key_type = 'secret'::key_type
      AND sk.is_active = TRUE
      AND a.is_active = TRUE
    LIMIT 1;
$$;