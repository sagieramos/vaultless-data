-- Migration: Fix ambiguous column references in add_publishable_key and deactivate_publishable_key functions
-- Date: 2026-02-05
-- Description: Add table aliases to disambiguate column references

-- ============================================================================
-- Function: add_publishable_key
-- Description: Add a new publishable key to an application
-- ============================================================================
CREATE OR REPLACE FUNCTION add_publishable_key(
    p_application_id UUID,
    p_user_id UUID,
    p_publishable_key_plaintext VARCHAR(128),
    p_key_prefix VARCHAR(32),
    p_max_keys INTEGER DEFAULT 5
)
RETURNS TABLE (
    new_key_id UUID,
    key_prefix VARCHAR(32),
    created_at TIMESTAMPTZ,
    total_active_publishable_keys BIGINT
)
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    v_app_id UUID;
    v_app_name VARCHAR(255);
    v_is_active BOOLEAN;
    v_current_count BIGINT;
    v_new_key_id UUID;
    v_created_at TIMESTAMPTZ;
BEGIN
    -- 1. Verify application exists, belongs to user, and is active
    SELECT a.id, a.name, a.is_active
    INTO v_app_id, v_app_name, v_is_active
    FROM applications a
    WHERE a.id = p_application_id
      AND a.developer_id = p_user_id;

    IF v_app_id IS NULL THEN
        RAISE EXCEPTION 'Application not found or access denied';
    END IF;

    IF NOT v_is_active THEN
        RAISE EXCEPTION 'Cannot add keys to inactive application';
    END IF;

    -- 2. Check current publishable key count
    SELECT COUNT(*)
    INTO v_current_count
    FROM api_keys ak
    WHERE ak.application_id = p_application_id
      AND ak.key_type = 'publishable'::key_type
      AND ak.is_active = true;

    IF v_current_count >= p_max_keys THEN
        RAISE EXCEPTION 'Maximum of % active publishable keys allowed per application', p_max_keys;
    END IF;

    -- 3. Insert new publishable key
    INSERT INTO api_keys (
        id,
        user_id,
        key_hash,
        key_prefix,
        description,
        scopes,
        is_active,
        created_at,
        updated_at,
        expires_at,
        last_used_at,
        application_id,
        key_type,
        publishable_key_plaintext
    )
    VALUES (
        uuid_generate_v4(),
        p_user_id,
        NULL,
        p_key_prefix,
        'Publishable key for ' || v_app_name || ' (additional)',
        NULL,
        true,
        NOW(),
        NOW(),
        NULL,
        NULL,
        p_application_id,
        'publishable'::key_type,
        p_publishable_key_plaintext
    )
    RETURNING api_keys.id, api_keys.created_at INTO v_new_key_id, v_created_at;

    -- 4. Return result
    RETURN QUERY
    SELECT
        v_new_key_id,
        p_key_prefix,
        v_created_at,
        v_current_count + 1;
END;
$$;

-- ============================================================================
-- Function: deactivate_publishable_key
-- Description: Deactivate a specific publishable key
-- ============================================================================
CREATE OR REPLACE FUNCTION deactivate_publishable_key(
    p_application_id UUID,
    p_user_id UUID,
    p_publishable_key_plaintext VARCHAR(128)
)
RETURNS TABLE (
    deactivated_key_id UUID,
    remaining_active_keys BIGINT
)
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    v_app_id UUID;
    v_is_active BOOLEAN;
    v_key_id UUID;
    v_remaining_count BIGINT;
BEGIN
    -- 1. Verify application exists, belongs to user, and is active
    SELECT a.id, a.is_active
    INTO v_app_id, v_is_active
    FROM applications a
    WHERE a.id = p_application_id
      AND a.developer_id = p_user_id;

    IF v_app_id IS NULL THEN
        RAISE EXCEPTION 'Application not found or access denied';
    END IF;

    IF NOT v_is_active THEN
        RAISE EXCEPTION 'Cannot deactivate keys for inactive application';
    END IF;

    -- 2. Find and deactivate the key
    UPDATE api_keys ak
    SET is_active = false,
        updated_at = NOW()
    WHERE ak.publishable_key_plaintext = p_publishable_key_plaintext
      AND ak.application_id = p_application_id
      AND ak.key_type = 'publishable'::key_type
      AND ak.is_active = true
    RETURNING ak.id INTO v_key_id;

    IF v_key_id IS NULL THEN
        RAISE EXCEPTION 'Publishable key not found or already inactive';
    END IF;

    -- 3. Count remaining active publishable keys
    SELECT COUNT(*)
    INTO v_remaining_count
    FROM api_keys ak
    WHERE ak.application_id = p_application_id
      AND ak.key_type = 'publishable'::key_type
      AND ak.is_active = true;

    -- 4. Return result
    RETURN QUERY
    SELECT
        v_key_id,
        v_remaining_count;
END;
$$;

-- Grant execute permissions
GRANT EXECUTE ON FUNCTION add_publishable_key(UUID, UUID, VARCHAR, VARCHAR, INTEGER) TO vaultless;
GRANT EXECUTE ON FUNCTION deactivate_publishable_key(UUID, UUID, VARCHAR) TO vaultless;
