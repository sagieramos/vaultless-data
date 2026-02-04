CREATE OR REPLACE FUNCTION update_application(
    p_app_id UUID,
    p_user_id UUID,
    p_name TEXT DEFAULT NULL,
    p_description TEXT DEFAULT NULL,
    p_is_active BOOLEAN DEFAULT NULL,
    p_max_ttl_seconds INT DEFAULT NULL,
    p_is_key_rotation_forced BOOLEAN DEFAULT NULL,
    p_internal_notes TEXT DEFAULT NULL,
    p_integrity_patch JSONB DEFAULT NULL,
    p_webhooks JSONB DEFAULT NULL
)
RETURNS TABLE (
    application_id UUID,
    developer_id UUID,
    subscription_id UUID,
    name TEXT,
    description TEXT,
    is_active BOOLEAN,
    created_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ,
    max_ttl_seconds INT,
    is_key_rotation_forced BOOLEAN,
    deletion_requested_at TIMESTAMPTZ,
    internal_notes TEXT,
    app_meta JSONB
)
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE applications
    SET
        name = COALESCE(p_name, name),
        description = COALESCE(p_description, description),
        is_active = COALESCE(p_is_active, is_active),
        max_ttl_seconds = COALESCE(p_max_ttl_seconds, max_ttl_seconds),
        is_key_rotation_forced = COALESCE(p_is_key_rotation_forced, is_key_rotation_forced),
        internal_notes = COALESCE(p_internal_notes, internal_notes),
        app_meta = jsonb_merge_patch(app_meta, COALESCE(p_integrity_patch, '{}'::jsonb)),
        updated_at = NOW()
    WHERE id = p_app_id AND developer_id = p_user_id
    RETURNING
        id,
        developer_id,
        subscription_id,
        name,
        description,
        is_active,
        created_at,
        updated_at,
        max_ttl_seconds,
        is_key_rotation_forced,
        deletion_requested_at,
        internal_notes,
        app_meta
    INTO STRICT
        application_id,
        developer_id,
        subscription_id,
        name,
        description,
        is_active,
        created_at,
        updated_at,
        max_ttl_seconds,
        is_key_rotation_forced,
        deletion_requested_at,
        internal_notes,
        app_meta;

    IF p_webhooks IS NOT NULL THEN
        PERFORM sync_webhooks(p_app_id, p_webhooks);
    END IF;

    RETURN NEXT;
END;
$$;

-- Grant execute permission
GRANT EXECUTE ON FUNCTION update_application(UUID, UUID, TEXT, TEXT, BOOLEAN, INT, BOOLEAN, TEXT, JSONB, JSONB) TO vaultless;