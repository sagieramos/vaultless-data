-- Migration: Add E2EE support for group messaging
-- Run this migration to add encrypted_group_keys and key_version columns

-- ============================================================================
-- 1. Add E2EE columns to message_groups table
-- ============================================================================

ALTER TABLE message_groups 
ADD COLUMN IF NOT EXISTS encrypted_group_keys JSONB,
ADD COLUMN IF NOT EXISTS key_version INTEGER NOT NULL DEFAULT 1;

-- Add index for key version lookups
CREATE INDEX IF NOT EXISTS idx_groups_key_version 
ON message_groups(key_version) 
WHERE is_active = true;

-- Add comment explaining the JSON structure
COMMENT ON COLUMN message_groups.encrypted_group_keys IS 
'JSON structure: {"keys": [{"client_id": "uuid", "encrypted_key": "base64", "key_version": 1, "encrypted_at": "timestamp"}]}';

COMMENT ON COLUMN message_groups.key_version IS 
'Incremented each time group key is rotated. Used for forward secrecy.';

-- ============================================================================
-- 2. Make recipient_client_id nullable for group messages
-- ============================================================================

-- Drop the foreign key constraint if it exists
ALTER TABLE messages 
DROP CONSTRAINT IF EXISTS messages_recipient_client_id_fkey;

-- Make the column nullable
ALTER TABLE messages 
ALTER COLUMN recipient_client_id DROP NOT NULL;

-- Re-add foreign key with ON DELETE CASCADE
ALTER TABLE messages
ADD CONSTRAINT messages_recipient_client_id_fkey 
FOREIGN KEY (recipient_client_id)
REFERENCES clients (id) 
ON DELETE CASCADE;

-- Update the check constraint to allow NULL recipient for group messages
ALTER TABLE messages 
DROP CONSTRAINT IF EXISTS chk_group_message_consistency;

ALTER TABLE messages
ADD CONSTRAINT chk_group_message_consistency 
CHECK (
    (is_group_message = false AND group_id IS NULL AND recipient_client_id IS NOT NULL)
    OR 
    (is_group_message = true AND group_id IS NOT NULL AND recipient_client_id IS NULL)
);

COMMENT ON CONSTRAINT chk_group_message_consistency ON messages IS
'Ensures group messages have group_id and NULL recipient, while direct messages have recipient_id and NULL group_id';

-- ============================================================================
-- 3. Create helper function to get encrypted key for a client
-- ============================================================================

CREATE OR REPLACE FUNCTION get_encrypted_group_key_for_client(
    p_group_id UUID,
    p_client_id UUID
) RETURNS JSONB
LANGUAGE plpgsql
STABLE
AS $$
DECLARE
    v_keys JSONB;
    v_key JSONB;
BEGIN
    -- Get encrypted_group_keys from the group
    SELECT encrypted_group_keys INTO v_keys
    FROM message_groups
    WHERE id = p_group_id;

    -- If no keys found, return NULL
    IF v_keys IS NULL THEN
        RETURN NULL;
    END IF;

    -- Search for the client's key in the keys array
    FOR v_key IN SELECT * FROM jsonb_array_elements(v_keys->'keys')
    LOOP
        IF (v_key->>'client_id')::UUID = p_client_id THEN
            RETURN v_key;
        END IF;
    END LOOP;

    -- Key not found for this client
    RETURN NULL;
END;
$$;

COMMENT ON FUNCTION get_encrypted_group_key_for_client IS
'Returns the encrypted group key for a specific client in a group. Used by clients to decrypt group messages.';

-- ============================================================================
-- 4. Create function to rotate group key (increment version)
-- ============================================================================

CREATE OR REPLACE FUNCTION rotate_group_encryption_key(
    p_group_id UUID,
    p_new_encrypted_keys JSONB
) RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_new_version INTEGER;
BEGIN
    -- Increment key version and update encrypted keys
    UPDATE message_groups
    SET 
        key_version = key_version + 1,
        encrypted_group_keys = p_new_encrypted_keys,
        updated_at = NOW()
    WHERE id = p_group_id
    RETURNING key_version INTO v_new_version;

    IF NOT FOUND THEN
        RAISE EXCEPTION 'Group not found: %', p_group_id;
    END IF;

    RETURN v_new_version;
END;
$$;

COMMENT ON FUNCTION rotate_group_encryption_key IS
'Rotates the group encryption key by incrementing version and updating all member keys. Should be called after member removal.';

-- ============================================================================
-- 5. Create trigger to suggest key rotation when member leaves
-- ============================================================================

CREATE OR REPLACE FUNCTION check_group_key_rotation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    -- When a member leaves, removed, or banned, log a notification
    -- (In production, you might want to send this to a queue or notification system)
    IF (NEW.status IN ('left', 'removed', 'banned')) AND 
       (OLD.status = 'active') THEN
        
        -- Insert into a notifications table or log
        RAISE NOTICE 'Member % left group %. Consider rotating group encryption key.', 
            NEW.client_address, NEW.group_id;
        
        -- You could also insert into a key_rotation_queue table here
        -- INSERT INTO key_rotation_queue (group_id, reason, created_at)
        -- VALUES (NEW.group_id, 'member_left', NOW());
    END IF;

    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trigger_suggest_key_rotation ON group_members;
CREATE TRIGGER trigger_suggest_key_rotation
    AFTER UPDATE ON group_members
    FOR EACH ROW
    WHEN (NEW.status IS DISTINCT FROM OLD.status)
    EXECUTE FUNCTION check_group_key_rotation();

COMMENT ON TRIGGER trigger_suggest_key_rotation ON group_members IS
'Logs a notification when a member status changes to suggest key rotation for security';

-- ============================================================================
-- 6. Create view for group key audit trail
-- ============================================================================

CREATE OR REPLACE VIEW group_key_audit AS
SELECT 
    g.id AS group_id,
    g.group_name,
    g.key_version,
    g.created_at AS group_created_at,
    g.updated_at AS last_key_update,
    g.member_count,
    COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'active') AS active_members,
    CASE 
        WHEN g.encrypted_group_keys IS NOT NULL THEN 
            jsonb_array_length(g.encrypted_group_keys->'keys')
        ELSE 0
    END AS encrypted_keys_count
FROM message_groups g
LEFT JOIN group_members m ON g.id = m.group_id
WHERE g.is_active = true
GROUP BY g.id, g.group_name, g.key_version, g.created_at, g.updated_at, 
         g.member_count, g.encrypted_group_keys;

COMMENT ON VIEW group_key_audit IS
'Audit view showing group key versions and member counts for security monitoring';

-- ============================================================================
-- 7. Example usage queries
-- ============================================================================

-- Get encrypted key for a specific client in a group
-- SELECT get_encrypted_group_key_for_client(
--     'group-uuid-here'::UUID,
--     'client-uuid-here'::UUID
-- );

-- Check which groups need key rotation (more keys than active members)
-- SELECT * FROM group_key_audit
-- WHERE encrypted_keys_count > active_members
-- ORDER BY last_key_update ASC;

-- Rotate group key (call from application)
-- SELECT rotate_group_encryption_key(
--     'group-uuid-here'::UUID,
--     '{"keys": [{"client_id": "...", "encrypted_key": "...", "key_version": 2, "encrypted_at": "2024-..."}]}'::JSONB
-- );

-- ============================================================================
-- 8. Create indexes for performance
-- ============================================================================

-- Index for looking up groups by key version (useful for key rotation monitoring)
CREATE INDEX IF NOT EXISTS idx_groups_key_version_updated 
ON message_groups(key_version, updated_at DESC)
WHERE is_active = true;

-- Index for message retrieval by group (already exists, but adding comment)
COMMENT ON INDEX idx_messages_group IS
'Optimizes group message retrieval. Critical for E2EE group messaging performance.';

-- ============================================================================
-- 9. Add constraints for data integrity
-- ============================================================================

-- Ensure key_version is always positive
ALTER TABLE message_groups
ADD CONSTRAINT chk_positive_key_version 
CHECK (key_version > 0);

-- Add check to ensure encrypted_group_keys has correct structure when present
CREATE OR REPLACE FUNCTION validate_encrypted_keys_structure()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.encrypted_group_keys IS NOT NULL THEN
        -- Check that it has a 'keys' array
        IF NOT (NEW.encrypted_group_keys ? 'keys') THEN
            RAISE EXCEPTION 'encrypted_group_keys must contain a "keys" array';
        END IF;
        
        -- Check that 'keys' is actually an array
        IF jsonb_typeof(NEW.encrypted_group_keys->'keys') != 'array' THEN
            RAISE EXCEPTION 'encrypted_group_keys["keys"] must be an array';
        END IF;
    END IF;
    
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trigger_validate_encrypted_keys ON message_groups;
CREATE TRIGGER trigger_validate_encrypted_keys
    BEFORE INSERT OR UPDATE ON message_groups
    FOR EACH ROW
    WHEN (NEW.encrypted_group_keys IS NOT NULL)
    EXECUTE FUNCTION validate_encrypted_keys_structure();

COMMENT ON TRIGGER trigger_validate_encrypted_keys ON message_groups IS
'Validates the structure of encrypted_group_keys JSON to prevent malformed data';

-- ============================================================================
-- 10. Update existing groups with default values
-- ============================================================================

-- For existing groups without encrypted_group_keys, set to empty structure
UPDATE message_groups
SET encrypted_group_keys = '{"keys": []}'::JSONB
WHERE encrypted_group_keys IS NULL;

-- ============================================================================
-- Rollback script (if needed)
-- ============================================================================

/*
-- To rollback this migration, run:

-- Drop triggers
DROP TRIGGER IF EXISTS trigger_suggest_key_rotation ON group_members;
DROP TRIGGER IF EXISTS trigger_validate_encrypted_keys ON message_groups;

-- Drop functions
DROP FUNCTION IF EXISTS check_group_key_rotation();
DROP FUNCTION IF EXISTS validate_encrypted_keys_structure();
DROP FUNCTION IF EXISTS rotate_group_encryption_key(UUID, JSONB);
DROP FUNCTION IF EXISTS get_encrypted_group_key_for_client(UUID, UUID);

-- Drop view
DROP VIEW IF EXISTS group_key_audit;

-- Drop indexes
DROP INDEX IF EXISTS idx_groups_key_version;
DROP INDEX IF EXISTS idx_groups_key_version_updated;

-- Remove constraints
ALTER TABLE message_groups DROP CONSTRAINT IF EXISTS chk_positive_key_version;
ALTER TABLE messages DROP CONSTRAINT IF EXISTS chk_group_message_consistency;

-- Remove columns
ALTER TABLE message_groups DROP COLUMN IF EXISTS encrypted_group_keys;
ALTER TABLE message_groups DROP COLUMN IF EXISTS key_version;

-- Restore original constraint
ALTER TABLE messages
ADD CONSTRAINT chk_group_message_consistency 
CHECK (
    (group_id IS NULL AND is_group_message = false) 
    OR 
    (group_id IS NOT NULL AND is_group_message = true)
);
*/