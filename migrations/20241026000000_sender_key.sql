-- ============================================================================
-- Migration: Complete E2EE Group Messaging with Advanced Features
-- ============================================================================
-- Features:
-- 1. Sender Keys Protocol support
-- 2. Encrypted message reactions
-- 3. Encrypted file sharing
-- 4. Enhanced group encryption

-- ============================================================================
-- 1. Add Sender Keys support to message_groups
-- ============================================================================

ALTER TABLE message_groups 
ADD COLUMN IF NOT EXISTS uses_sender_keys BOOLEAN NOT NULL DEFAULT false;

COMMENT ON COLUMN message_groups.uses_sender_keys IS
'True: use Sender Keys Protocol (efficient for large groups). False: use shared group key';

-- ============================================================================
-- 2. Add Sender Keys columns to group_members
-- ============================================================================

ALTER TABLE group_members
ADD COLUMN IF NOT EXISTS sender_chain_public_key TEXT,
ADD COLUMN IF NOT EXISTS sender_key_version INTEGER NOT NULL DEFAULT 1;

CREATE INDEX IF NOT EXISTS idx_group_members_sender_key 
ON group_members(group_id, client_address, sender_key_version)
WHERE sender_chain_public_key IS NOT NULL;

COMMENT ON COLUMN group_members.sender_chain_public_key IS
'Public signing key for Sender Keys Protocol. Each sender has their own chain key.';

-- ============================================================================
-- 3. Create sender_keys table (Sender Keys Protocol)
-- ============================================================================

CREATE TABLE IF NOT EXISTS sender_keys (
    id UUID NOT NULL DEFAULT uuid_generate_v4(),
    group_id UUID NOT NULL,
    sender_client_id UUID NOT NULL,
    recipient_client_id UUID NOT NULL,
    encrypted_chain_key TEXT NOT NULL,
    key_version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    CONSTRAINT sender_keys_pkey PRIMARY KEY (id),
    CONSTRAINT sender_keys_unique UNIQUE (group_id, sender_client_id, recipient_client_id),
    CONSTRAINT sender_keys_group_fkey FOREIGN KEY (group_id)
        REFERENCES message_groups (id) ON DELETE CASCADE,
    CONSTRAINT sender_keys_sender_fkey FOREIGN KEY (sender_client_id)
        REFERENCES clients (id) ON DELETE CASCADE,
    CONSTRAINT sender_keys_recipient_fkey FOREIGN KEY (recipient_client_id)
        REFERENCES clients (id) ON DELETE CASCADE
);

CREATE INDEX idx_sender_keys_recipient ON sender_keys(recipient_client_id, group_id);
CREATE INDEX idx_sender_keys_sender ON sender_keys(sender_client_id, group_id);
CREATE INDEX idx_sender_keys_version ON sender_keys(key_version);

COMMENT ON TABLE sender_keys IS
'Stores encrypted chain keys for Sender Keys Protocol. Each sender maintains keys for all recipients.';

-- ============================================================================
-- 4. Create message_reactions table (Encrypted Reactions)
-- ============================================================================

CREATE TABLE IF NOT EXISTS message_reactions (
    id UUID NOT NULL DEFAULT uuid_generate_v4(),
    message_id UUID NOT NULL,
    client_id UUID NOT NULL,
    encrypted_reaction TEXT NOT NULL,  -- Emoji/reaction encrypted with message key
    nonce VARCHAR(32) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    CONSTRAINT message_reactions_pkey PRIMARY KEY (id),
    CONSTRAINT message_reactions_unique UNIQUE (message_id, client_id, encrypted_reaction),
    CONSTRAINT message_reactions_message_fkey FOREIGN KEY (message_id)
        REFERENCES messages (id) ON DELETE CASCADE,
    CONSTRAINT message_reactions_client_fkey FOREIGN KEY (client_id)
        REFERENCES clients (id) ON DELETE CASCADE
);

CREATE INDEX idx_reactions_message ON message_reactions(message_id);
CREATE INDEX idx_reactions_client ON message_reactions(client_id);
CREATE INDEX idx_reactions_created ON message_reactions(created_at DESC);

COMMENT ON TABLE message_reactions IS
'Encrypted reactions to messages. Reactions are encrypted with the same key as the message.';

-- ============================================================================
-- 5. Create group_files table (Encrypted File Sharing)
-- ============================================================================

CREATE TABLE IF NOT EXISTS group_files (
    id UUID NOT NULL DEFAULT uuid_generate_v4(),
    group_id UUID NOT NULL,
    message_id UUID,  -- Optional: link to a message
    uploader_client_id UUID NOT NULL,
    
    -- Encrypted metadata
    encrypted_filename TEXT NOT NULL,
    encrypted_mime_type TEXT NOT NULL,
    file_size_bytes BIGINT NOT NULL,
    
    -- Encryption
    encrypted_file_key TEXT NOT NULL,  -- File key encrypted with group key
    nonce VARCHAR(32) NOT NULL,
    
    -- Storage
    storage_path TEXT NOT NULL,
    chunk_count INTEGER NOT NULL DEFAULT 1,
    
    -- Access control
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE,
    download_count INTEGER NOT NULL DEFAULT 0,
    max_downloads INTEGER,
    
    CONSTRAINT group_files_pkey PRIMARY KEY (id),
    CONSTRAINT group_files_group_fkey FOREIGN KEY (group_id)
        REFERENCES message_groups (id) ON DELETE CASCADE,
    CONSTRAINT group_files_message_fkey FOREIGN KEY (message_id)
        REFERENCES messages (id) ON DELETE CASCADE,
    CONSTRAINT group_files_uploader_fkey FOREIGN KEY (uploader_client_id)
        REFERENCES clients (id) ON DELETE CASCADE,
    CONSTRAINT valid_file_size CHECK (file_size_bytes > 0),
    CONSTRAINT valid_chunk_count CHECK (chunk_count > 0),
    CONSTRAINT valid_download_count CHECK (download_count >= 0),
    CONSTRAINT valid_max_downloads CHECK (max_downloads IS NULL OR max_downloads > 0)
);

CREATE INDEX idx_files_group ON group_files(group_id, created_at DESC);
CREATE INDEX idx_files_uploader ON group_files(uploader_client_id, created_at DESC);
CREATE INDEX idx_files_expires ON group_files(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_files_message ON group_files(message_id) WHERE message_id IS NOT NULL;

COMMENT ON TABLE group_files IS
'Encrypted files shared in groups. Files use separate encryption keys for performance.';

-- ============================================================================
-- 6. Create file_chunks table (For Large Files)
-- ============================================================================

CREATE TABLE IF NOT EXISTS file_chunks (
    id UUID NOT NULL DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL,
    chunk_index INTEGER NOT NULL,
    encrypted_data BYTEA NOT NULL,
    chunk_size_bytes INTEGER NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    CONSTRAINT file_chunks_pkey PRIMARY KEY (id),
    CONSTRAINT file_chunks_unique UNIQUE (file_id, chunk_index),
    CONSTRAINT file_chunks_file_fkey FOREIGN KEY (file_id)
        REFERENCES group_files (id) ON DELETE CASCADE,
    CONSTRAINT valid_chunk_index CHECK (chunk_index >= 0),
    CONSTRAINT valid_chunk_size CHECK (chunk_size_bytes > 0)
);

CREATE INDEX idx_chunks_file ON file_chunks(file_id, chunk_index);

COMMENT ON TABLE file_chunks IS
'Stores encrypted chunks for large files (> 10MB). Allows streaming and partial downloads.';

-- ============================================================================
-- 7. Triggers for automatic cleanup
-- ============================================================================

-- Trigger to clean up sender keys when member leaves
CREATE OR REPLACE FUNCTION cleanup_sender_keys_on_member_leave()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status IN ('left', 'removed', 'banned') AND OLD.status = 'active' THEN
        -- Delete sender keys for this member
        DELETE FROM sender_keys
        WHERE group_id = NEW.group_id 
            AND (sender_client_id = NEW.client_address 
                 OR recipient_client_id = NEW.client_address);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_cleanup_sender_keys ON group_members;
CREATE TRIGGER trigger_cleanup_sender_keys
    AFTER UPDATE ON group_members
    FOR EACH ROW
    WHEN (NEW.status IS DISTINCT FROM OLD.status)
    EXECUTE FUNCTION cleanup_sender_keys_on_member_leave();

-- Trigger to clean up reactions when message is deleted
CREATE OR REPLACE FUNCTION cleanup_reactions_on_message_delete()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM message_reactions WHERE message_id = OLD.id;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_cleanup_reactions ON messages;
CREATE TRIGGER trigger_cleanup_reactions
    BEFORE DELETE ON messages
    FOR EACH ROW
    EXECUTE FUNCTION cleanup_reactions_on_message_delete();

-- ============================================================================
-- 8. Functions for common operations
-- ============================================================================

-- Get sender key for recipient
CREATE OR REPLACE FUNCTION get_sender_key_for_recipient(
    p_group_id UUID,
    p_sender_id UUID,
    p_recipient_id UUID
) RETURNS TABLE (
    encrypted_chain_key TEXT,
    key_version INTEGER,
    signing_key TEXT
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        sk.encrypted_chain_key,
        sk.key_version,
        gm.sender_chain_public_key
    FROM sender_keys sk
    INNER JOIN group_members gm ON sk.sender_client_id = gm.client_address 
        AND sk.group_id = gm.group_id
    WHERE sk.group_id = p_group_id
        AND sk.sender_client_id = p_sender_id
        AND sk.recipient_client_id = p_recipient_id
        AND gm.status = 'active'
    ORDER BY sk.key_version DESC
    LIMIT 1;
END;
$$ LANGUAGE plpgsql STABLE;

-- Get reaction summary for a message
CREATE OR REPLACE FUNCTION get_reaction_summary(
    p_message_id UUID,
    p_client_id UUID DEFAULT NULL
) RETURNS TABLE (
    encrypted_reaction TEXT,
    reaction_count BIGINT,
    reacted_by_me BOOLEAN
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        mr.encrypted_reaction,
        COUNT(*)::BIGINT as reaction_count,
        BOOL_OR(mr.client_id = p_client_id) as reacted_by_me
    FROM message_reactions mr
    WHERE mr.message_id = p_message_id
    GROUP BY mr.encrypted_reaction
    ORDER BY reaction_count DESC;
END;
$$ LANGUAGE plpgsql STABLE;

-- Get files for a group with pagination
CREATE OR REPLACE FUNCTION get_group_files_paginated(
    p_group_id UUID,
    p_limit INTEGER DEFAULT 20,
    p_offset INTEGER DEFAULT 0
) RETURNS TABLE (
    file_id UUID,
    encrypted_filename TEXT,
    file_size_bytes BIGINT,
    uploader_client_id UUID,
    created_at TIMESTAMP WITH TIME ZONE,
    download_count INTEGER,
    total_count BIGINT
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        gf.id,
        gf.encrypted_filename,
        gf.file_size_bytes,
        gf.uploader_client_id,
        gf.created_at,
        gf.download_count,
        COUNT(*) OVER() as total_count
    FROM group_files gf
    WHERE gf.group_id = p_group_id
        AND (gf.expires_at IS NULL OR gf.expires_at > NOW())
    ORDER BY gf.created_at DESC
    LIMIT p_limit
    OFFSET p_offset;
END;
$$ LANGUAGE plpgsql STABLE;

-- ============================================================================
-- 9. Views for analytics
-- ============================================================================

-- Group activity summary
CREATE OR REPLACE VIEW group_activity_summary AS
SELECT 
    g.id AS group_id,
    g.group_name,
    g.member_count,
    g.message_count,
    COUNT(DISTINCT gf.id) AS file_count,
    SUM(gf.file_size_bytes) AS total_file_size_bytes,
    COUNT(DISTINCT mr.id) AS total_reactions,
    g.last_message_at,
    g.created_at
FROM message_groups g
LEFT JOIN group_files gf ON g.id = gf.group_id 
    AND (gf.expires_at IS NULL OR gf.expires_at > NOW())
LEFT JOIN messages m ON g.id = m.group_id
LEFT JOIN message_reactions mr ON m.id = mr.message_id
WHERE g.is_active = true
GROUP BY g.id;

-- Popular files
CREATE OR REPLACE VIEW popular_group_files AS
SELECT 
    gf.id,
    gf.group_id,
    gf.encrypted_filename,
    gf.file_size_bytes,
    gf.download_count,
    gf.created_at,
    g.group_name
FROM group_files gf
INNER JOIN message_groups g ON gf.group_id = g.id
WHERE gf.expires_at IS NULL OR gf.expires_at > NOW()
ORDER BY gf.download_count DESC;

-- ============================================================================
-- 10. Scheduled cleanup jobs (use with pg_cron or application scheduler)
-- ============================================================================

-- Function to cleanup expired files and chunks
CREATE OR REPLACE FUNCTION cleanup_expired_files()
RETURNS TABLE (
    deleted_files BIGINT,
    deleted_chunks BIGINT
) AS $$
DECLARE
    v_deleted_files BIGINT;
    v_deleted_chunks BIGINT;
BEGIN
    -- Delete expired files
    WITH deleted AS (
        DELETE FROM group_files
        WHERE expires_at < NOW()
        RETURNING id
    )
    SELECT COUNT(*) INTO v_deleted_files FROM deleted;
    
    -- Delete orphaned chunks (files that were deleted)
    WITH deleted AS (
        DELETE FROM file_chunks
        WHERE file_id NOT IN (SELECT id FROM group_files)
        RETURNING id
    )
    SELECT COUNT(*) INTO v_deleted_chunks FROM deleted;
    
    RETURN QUERY SELECT v_deleted_files, v_deleted_chunks;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- 11. Performance indexes
-- ============================================================================

-- Composite index for sender keys lookup (most common query)
CREATE INDEX IF NOT EXISTS idx_sender_keys_lookup 
ON sender_keys(group_id, recipient_client_id, sender_client_id, key_version DESC);

-- Index for reaction aggregation
CREATE INDEX IF NOT EXISTS idx_reactions_aggregation 
ON message_reactions(message_id, encrypted_reaction);

-- Index for file downloads
CREATE INDEX IF NOT EXISTS idx_files_downloads 
ON group_files(download_count DESC, created_at DESC);

-- ============================================================================
-- 12. Example usage queries
-- ============================================================================

-- Get sender key for decrypting a message
/*
SELECT * FROM get_sender_key_for_recipient(
    'group-uuid'::UUID,
    'sender-uuid'::UUID,
    'my-client-uuid'::UUID
);
*/

-- Get reaction summary for a message
/*
SELECT * FROM get_reaction_summary(
    'message-uuid'::UUID,
    'my-client-uuid'::UUID
);
*/

-- Get paginated files for a group
/*
SELECT * FROM get_group_files_paginated(
    'group-uuid'::UUID,
    20,  -- limit
    0    -- offset
);
*/

-- Cleanup expired files (run daily)
/*
SELECT * FROM cleanup_expired_files();
*/

-- ============================================================================
-- Rollback Script
-- ============================================================================

/*
-- To rollback this migration:

-- Drop views
DROP VIEW IF EXISTS group_activity_summary CASCADE;
DROP VIEW IF EXISTS popular_group_files CASCADE;

-- Drop functions
DROP FUNCTION IF EXISTS get_sender_key_for_recipient(UUID, UUID, UUID);
DROP FUNCTION IF EXISTS get_reaction_summary(UUID, UUID);
DROP FUNCTION IF EXISTS get_group_files_paginated(UUID, INTEGER, INTEGER);
DROP FUNCTION IF EXISTS cleanup_expired_files();
DROP FUNCTION IF EXISTS cleanup_sender_keys_on_member_leave();
DROP FUNCTION IF EXISTS cleanup_reactions_on_message_delete();

-- Drop triggers
DROP TRIGGER IF EXISTS trigger_cleanup_sender_keys ON group_members;
DROP TRIGGER IF EXISTS trigger_cleanup_reactions ON messages;

-- Drop tables (in correct order due to foreign keys)
DROP TABLE IF EXISTS file_chunks CASCADE;
DROP TABLE IF EXISTS group_files CASCADE;
DROP TABLE IF EXISTS message_reactions CASCADE;
DROP TABLE IF EXISTS sender_keys CASCADE;

-- Remove columns
ALTER TABLE group_members DROP COLUMN IF EXISTS sender_chain_public_key;
ALTER TABLE group_members DROP COLUMN IF EXISTS sender_key_version;
ALTER TABLE message_groups DROP COLUMN IF EXISTS uses_sender_keys;
*/