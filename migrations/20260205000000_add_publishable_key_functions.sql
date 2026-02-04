-- ============================================================================
-- Migration: Add PostgreSQL functions for publishable key management
-- Description: Moves add_publishable_key and deactivate_publishable_key
--              logic from application layer to database layer
-- ============================================================================

BEGIN;

-- ============================================================================
-- Function: add_publishable_key
-- Description: Add a new publishable key to an application
-- ============================================================================
CREATE OR REPLACE FUNCTION add_publishable_key(
    p_application_id UUID,
    p_user_id UUID,
    p_publishable_key_plaintext VARCHAR(64),
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
    FROM api_keys
    WHERE application_id = p_application_id
      AND key_type = 'publishable'
      AND is_active = true;

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
        NULL,
        NULL,
        p_application_id,
        'publishable',
        p_publishable_key_plaintext
    )
    RETURNING id, created_at INTO v_new_key_id, v_created_at;

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
-- Description: Deactivate a specific publishable key by its plaintext value
-- ============================================================================
CREATE OR REPLACE FUNCTION deactivate_publishable_key(
    p_application_id UUID,
    p_user_id UUID,
    p_publishable_key_plaintext VARCHAR(64)
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
    v_active_count BIGINT;
    v_key_id UUID;
BEGIN
    -- 1. Verify application exists and belongs to user
    SELECT id
    INTO v_app_id
    FROM applications
    WHERE id = p_application_id
      AND developer_id = p_user_id;

    IF v_app_id IS NULL THEN
        RAISE EXCEPTION 'Application not found or access denied';
    END IF;

    -- 2. Check how many active publishable keys exist
    SELECT COUNT(*)
    INTO v_active_count
    FROM api_keys
    WHERE application_id = p_application_id
      AND key_type = 'publishable'
      AND is_active = true;

    IF v_active_count <= 1 THEN
        RAISE EXCEPTION 'Cannot deactivate the last active publishable key. Use rotate instead.';
    END IF;

    -- 3. Fetch the key to deactivate by publishable_key_plaintext
    SELECT id
    INTO v_key_id
    FROM api_keys
    WHERE publishable_key_plaintext = p_publishable_key_plaintext
      AND application_id = p_application_id
      AND key_type = 'publishable'
      AND is_active = true;

    IF v_key_id IS NULL THEN
        RAISE EXCEPTION 'Specified publishable key not found or already inactive';
    END IF;

    -- 4. Deactivate the key
    UPDATE api_keys
    SET is_active = false,
        updated_at = NOW()
    WHERE id = v_key_id;

    -- 5. Return result
    RETURN QUERY
    SELECT
        v_key_id,
        v_active_count - 1;
END;
$$;

-- Grant execute permissions
GRANT EXECUTE ON FUNCTION add_publishable_key(UUID, UUID, VARCHAR, VARCHAR, INTEGER) TO vaultless;
GRANT EXECUTE ON FUNCTION deactivate_publishable_key(UUID, UUID, VARCHAR) TO vaultless;

COMMIT;

-- migrate:down
-- DROP FUNCTION IF EXISTS add_publishable_key(UUID, UUID, VARCHAR, VARCHAR, INTEGER);
-- DROP FUNCTION IF EXISTS deactivate_publishable_key(UUID, UUID, VARCHAR);
