-- ============================================================================
-- Migration: Remove subscription_entitlements and restore create_application
-- ============================================================================

BEGIN;

DROP TABLE IF EXISTS public.subscription_entitlements CASCADE;
DROP TABLE IF EXISTS public.subscription_snapshots CASCADE;

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

    -- Create free subscription for this application (expires in 30 days)
    INSERT INTO developer_subscriptions (
        developer_id, application_id, tier,
        current_period_start, current_period_end
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

COMMIT;
