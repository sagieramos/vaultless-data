-- Migration: Add create_application PostgreSQL function

-- Function to create a new application with secret and publishable keys
CREATE OR REPLACE FUNCTION create_application(
    p_user_id UUID,
    p_name TEXT,
    p_description TEXT DEFAULT NULL,
    p_max_ttl_seconds INTEGER DEFAULT 604800,
    p_is_key_rotation_forced BOOLEAN DEFAULT FALSE,
    p_environment TEXT DEFAULT 'live'
)
RETURNS TABLE (
    application_id UUID,
    user_id UUID,
    subscription_id UUID,
    name TEXT,
    description TEXT,
    is_active BOOLEAN,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ,
    max_ttl_seconds INTEGER,
    is_key_rotation_forced BOOLEAN,
    deletion_requested_at TIMESTAMPTZ,
    internal_notes TEXT,
    app_meta JSONB,
    secret_key_prefix TEXT,
    publishable_key_plaintext TEXT
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_app_id UUID;
    v_secret_key TEXT;
    v_secret_key_hash TEXT;
    v_secret_key_prefix TEXT;
    v_publishable_key TEXT;
    v_pk_prefix TEXT;
    v_app_row applications%ROWTYPE;
BEGIN
    -- Validate inputs
    IF LENGTH(TRIM(p_name)) = 0 THEN
        RAISE EXCEPTION 'Application name cannot be empty';
    END IF;

    IF p_max_ttl_seconds IS NOT NULL AND p_max_ttl_seconds <= 0 THEN
        RAISE EXCEPTION 'Max TTL seconds must be positive';
    END IF;

    -- Generate application ID
    v_app_id := gen_random_uuid();

    -- Create the application
    INSERT INTO applications (
        id,
        developer_id,
        name,
        description,
        max_ttl_seconds,
        is_key_rotation_forced
    )
    VALUES (
        v_app_id,
        p_user_id,
        p_name,
        p_description,
        COALESCE(p_max_ttl_seconds, 604800),
        COALESCE(p_is_key_rotation_forced, FALSE)
    )
    RETURNING * INTO v_app_row;

    -- Generate and create secret key
    v_secret_key := 'sk_' || p_environment || '_' || encode(gen_random_bytes(32), 'hex');
    v_secret_key_hash := encode(digest(v_secret_key, 'sha256'), 'hex');
    v_secret_key_prefix := LEFT(v_secret_key, 8);

    INSERT INTO api_keys (
        id,
        user_id,
        key_hash,
        key_prefix,
        description,
        is_active,
        created_at,
        updated_at,
        application_id,
        key_type
    )
    VALUES (
        gen_random_uuid(),  -- id
        p_user_id,          -- user_id
        v_secret_key_hash,  -- key_hash (hashed for security)
        v_secret_key_prefix, -- key_prefix (first 8 chars of the actual key)
        'Secret key for ' || p_name, -- description
        TRUE,               -- is_active
        NOW(),              -- created_at
        NOW(),              -- updated_at
        v_app_id,           -- application_id
        'secret'::key_type_enum  -- key_type
    );

    -- Generate and create publishable key
    v_publishable_key := 'pk_' || p_environment || '_' || encode(gen_random_bytes(24), 'hex');
    v_pk_prefix := LEFT(v_publishable_key, 16);

    INSERT INTO api_keys (
        id,
        user_id,
        key_prefix,
        description,
        is_active,
        created_at,
        updated_at,
        application_id,
        key_type,
        publishable_key_plaintext
    )
    VALUES (
        gen_random_uuid(),  -- id
        p_user_id,          -- user_id
        v_pk_prefix,        -- key_prefix
        'Publishable key for ' || p_name, -- description
        TRUE,               -- is_active
        NOW(),              -- created_at
        NOW(),              -- updated_at
        v_app_id,           -- application_id
        'publishable'::key_type_enum, -- key_type
        v_publishable_key   -- publishable_key_plaintext
    );

    -- Return the application and keys
    RETURN QUERY
    SELECT
        v_app_row.id,
        v_app_row.developer_id,
        v_app_row.subscription_id,
        v_app_row.name,
        v_app_row.description,
        v_app_row.is_active,
        v_app_row.created_at,
        v_app_row.updated_at,
        v_app_row.max_ttl_seconds,
        v_app_row.is_key_rotation_forced,
        v_app_row.deletion_requested_at,
        v_app_row.internal_notes,
        v_app_row.app_meta,
        v_secret_key_prefix::TEXT,  -- Return only the prefix for security
        v_publishable_key::TEXT;
END;
$$;

-- Grant execute permission
GRANT EXECUTE ON FUNCTION create_application(UUID, TEXT, TEXT, INTEGER, BOOLEAN, TEXT) TO vaultless;