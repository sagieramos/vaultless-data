-- migrations/20241022000003_group_messaging.sql
-- =======================================================================
-- Group messaging schema: enums, tables, indexes, triggers, helpers
-- =======================================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================
-- ENUM TYPES (if not exists)
-- ============================
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'group_type_enum') THEN
        CREATE TYPE group_type_enum AS ENUM ('private', 'public', 'broadcast');
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'member_role_enum') THEN
        CREATE TYPE member_role_enum AS ENUM ('admin', 'moderator', 'member');
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'member_status_enum') THEN
        CREATE TYPE member_status_enum AS ENUM ('active', 'muted', 'left', 'removed', 'banned');
    END IF;
END;
$$ LANGUAGE plpgsql;


-- ============================================================================
-- MESSAGE GROUPS TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS message_groups (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    -- Group identity
    group_name VARCHAR(255),
    group_type group_type_enum NOT NULL DEFAULT 'private',

    -- Creator (client only)
    creator_client_address UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,

    -- Group settings
    allow_member_invite BOOLEAN NOT NULL DEFAULT FALSE,
    require_admin_approval BOOLEAN NOT NULL DEFAULT TRUE,
    max_members INTEGER NOT NULL DEFAULT 100,

    -- Optional E2EE group public key (serialized, provider-defined)
    group_public_key TEXT,

    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    is_archived BOOLEAN NOT NULL DEFAULT FALSE,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_message_at TIMESTAMPTZ,

    -- Stats (enforced non-negative)
    member_count INTEGER NOT NULL DEFAULT 1 CHECK (member_count >= 0),
    message_count INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),

    -- Metadata
    metadata JSONB
);

CREATE INDEX IF NOT EXISTS idx_groups_creator ON message_groups(creator_client_address);
CREATE INDEX IF NOT EXISTS idx_groups_active ON message_groups(is_active) WHERE is_active = TRUE;
CREATE INDEX IF NOT EXISTS idx_groups_last_message ON message_groups(last_message_at DESC);


-- ============================================================================
-- GROUP MEMBERS TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS group_members (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    group_id UUID NOT NULL REFERENCES message_groups(id) ON DELETE CASCADE,
    client_address UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,

    -- Role and status
    role member_role_enum NOT NULL DEFAULT 'member',
    status member_status_enum NOT NULL DEFAULT 'active',

    -- Permissions
    can_send_messages BOOLEAN NOT NULL DEFAULT TRUE,
    can_add_members BOOLEAN NOT NULL DEFAULT FALSE,
    can_remove_members BOOLEAN NOT NULL DEFAULT FALSE,

    -- Tracking
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    left_at TIMESTAMPTZ,
    last_read_at TIMESTAMPTZ,
    unread_count INTEGER NOT NULL DEFAULT 0 CHECK (unread_count >= 0),

    -- Added by
    invited_by_client_address UUID,

    -- Metadata
    metadata JSONB,

    -- Unique member per group
    UNIQUE(group_id, client_address)
);

-- Prevent obvious self-invite
ALTER TABLE group_members
    ADD CONSTRAINT chk_invite_self
    CHECK (invited_by_client_address IS NULL OR invited_by_client_address <> client_address);

CREATE INDEX IF NOT EXISTS idx_group_members_group ON group_members(group_id, status);
CREATE INDEX IF NOT EXISTS idx_group_members_client ON group_members(client_address);
CREATE INDEX IF NOT EXISTS idx_group_members_active ON group_members(group_id, status)
    WHERE status = 'active';


-- ============================================================================
-- UPDATE MESSAGES TABLE FOR GROUPS
-- ============================================================================
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name='messages' AND column_name='group_id'
    ) THEN
        ALTER TABLE messages
            ADD COLUMN group_id UUID REFERENCES message_groups(id) ON DELETE CASCADE;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name='messages' AND column_name='is_group_message'
    ) THEN
        ALTER TABLE messages
            ADD COLUMN is_group_message BOOLEAN NOT NULL DEFAULT FALSE;
    END IF;
END;
$$ LANGUAGE plpgsql;

CREATE INDEX IF NOT EXISTS idx_messages_group ON messages(group_id, created_at DESC)
    WHERE group_id IS NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_group_message_consistency'
        AND conrelid = 'messages'::regclass
    ) THEN
        ALTER TABLE messages
        ADD CONSTRAINT chk_group_message_consistency CHECK (
            (group_id IS NULL AND is_group_message = FALSE)
            OR (group_id IS NOT NULL AND is_group_message = TRUE)
        );
    END IF;
END;
$$ LANGUAGE plpgsql;


-- ============================================================================
-- GROUP MESSAGE READ RECEIPTS
-- ============================================================================
CREATE TABLE IF NOT EXISTS group_message_read_receipts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    group_id UUID NOT NULL REFERENCES message_groups(id) ON DELETE CASCADE,
    client_address UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    read_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(message_id, client_address)
);

CREATE INDEX IF NOT EXISTS idx_read_receipts_message ON group_message_read_receipts(message_id);
CREATE INDEX IF NOT EXISTS idx_read_receipts_group_client ON group_message_read_receipts(group_id, client_address);


-- ============================================================================
-- TRIGGERS: update_group_member_count
-- ============================================================================
CREATE OR REPLACE FUNCTION update_group_member_count()
RETURNS TRIGGER AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.status = 'active' THEN
            UPDATE message_groups
            SET member_count = member_count + 1,
                updated_at = NOW()
            WHERE id = NEW.group_id;
        END IF;
    ELSIF TG_OP = 'UPDATE' THEN
        IF OLD.status = 'active' AND NEW.status != 'active' THEN
            UPDATE message_groups
            SET member_count = GREATEST(member_count - 1, 0),
                updated_at = NOW()
            WHERE id = NEW.group_id;
        ELSIF OLD.status != 'active' AND NEW.status = 'active' THEN
            UPDATE message_groups
            SET member_count = member_count + 1,
                updated_at = NOW()
            WHERE id = NEW.group_id;
        END IF;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_update_group_member_count ON group_members;
CREATE TRIGGER trigger_update_group_member_count
    AFTER INSERT OR UPDATE ON group_members
    FOR EACH ROW
    EXECUTE FUNCTION update_group_member_count();


-- ============================================================================
-- TRIGGERS: update_group_message_stats
-- ============================================================================
CREATE OR REPLACE FUNCTION update_group_message_stats()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.group_id IS NOT NULL THEN
        UPDATE message_groups
        SET
            last_message_at = GREATEST(NEW.created_at, COALESCE(last_message_at, to_timestamp(0))),
            message_count = message_count + 1,
            updated_at = NOW()
        WHERE id = NEW.group_id;

        UPDATE group_members
        SET unread_count = unread_count + 1
        WHERE group_id = NEW.group_id
            AND client_address != NEW.sender_client_address
            AND status = 'active';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_update_group_message_stats ON messages;
CREATE TRIGGER trigger_update_group_message_stats
    AFTER INSERT ON messages
    FOR EACH ROW
    WHEN (NEW.group_id IS NOT NULL)
    EXECUTE FUNCTION update_group_message_stats();


-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================
CREATE OR REPLACE FUNCTION create_message_group(
    p_creator_address UUID,
    p_group_name VARCHAR,
    p_group_type group_type_enum DEFAULT 'private'
)
RETURNS UUID AS $$
DECLARE
    v_group_id UUID;
BEGIN
    INSERT INTO message_groups (
        creator_client_address,
        group_name,
        group_type,
        created_at, updated_at
    )
    VALUES (p_creator_address, p_group_name, p_group_type, NOW(), NOW())
    RETURNING id INTO v_group_id;

    INSERT INTO group_members (
        group_id,
        client_address,
        role,
        can_add_members,
        can_remove_members,
        status,
        joined_at
    )
    VALUES (
        v_group_id,
        p_creator_address,
        'admin',
        TRUE,
        TRUE,
        'active',
        NOW()
    );

    UPDATE message_groups
    SET member_count = 1, updated_at = NOW()
    WHERE id = v_group_id;

    RETURN v_group_id;
END;
$$ LANGUAGE plpgsql;


CREATE OR REPLACE FUNCTION add_group_member(
    p_group_id UUID,
    p_client_address UUID,
    p_invited_by UUID
)
RETURNS UUID AS $$
DECLARE
    v_member_id UUID;
BEGIN
    INSERT INTO group_members (
        group_id,
        client_address,
        invited_by_client_address,
        status,
        joined_at
    )
    VALUES (p_group_id, p_client_address, p_invited_by, 'active', NOW())
    RETURNING id INTO v_member_id;

    RETURN v_member_id;
EXCEPTION WHEN unique_violation THEN
    SELECT id INTO v_member_id FROM group_members WHERE group_id = p_group_id AND client_address = p_client_address;
    RETURN v_member_id;
END;
$$ LANGUAGE plpgsql;


CREATE OR REPLACE FUNCTION is_group_member(
    p_group_id UUID,
    p_client_address UUID
)
RETURNS BOOLEAN AS $$
DECLARE
    v_exists BOOLEAN;
BEGIN
    SELECT EXISTS(
        SELECT 1 FROM group_members
        WHERE group_id = p_group_id
            AND client_address = p_client_address
            AND status = 'active'
    ) INTO v_exists;

    RETURN v_exists;
END;
$$ LANGUAGE plpgsql;


CREATE OR REPLACE FUNCTION get_group_member_addresses(p_group_id UUID)
RETURNS UUID[] AS $$
BEGIN
    RETURN ARRAY(
        SELECT client_address
        FROM group_members
        WHERE group_id = p_group_id
            AND status = 'active'
    );
END;
$$ LANGUAGE plpgsql;


CREATE OR REPLACE FUNCTION mark_group_message_read(
    p_message_id UUID,
    p_group_id UUID,
    p_client_address UUID
)
RETURNS VOID AS $$
BEGIN
    INSERT INTO group_message_read_receipts (
        message_id,
        group_id,
        client_address,
        read_at
    )
    VALUES (p_message_id, p_group_id, p_client_address, NOW())
    ON CONFLICT (message_id, client_address) DO NOTHING;

    UPDATE group_members
    SET unread_count = GREATEST(unread_count - 1, 0),
        last_read_at = NOW()
    WHERE group_id = p_group_id
        AND client_address = p_client_address;
END;
$$ LANGUAGE plpgsql;


-- ============================================================================
-- COMMENTS
-- ============================================================================
COMMENT ON TABLE message_groups IS 'Group chats and broadcast channels';
COMMENT ON TABLE group_members IS 'Group membership with roles and permissions';
COMMENT ON TABLE group_message_read_receipts IS 'Track who read which group messages';
COMMENT ON COLUMN group_members.unread_count IS 'Number of unread messages in this group';
