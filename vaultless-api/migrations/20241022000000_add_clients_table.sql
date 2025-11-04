-- migrations/20241022000000_add_clients_table.sql

-- ============================================================================
-- CLIENTS TABLE (Anonymous Ephemeral Identity)
-- Represents anonymous users with cryptographic identity
-- Zero personal information stored - pure privacy-first design
-- ============================================================================

CREATE TABLE IF NOT EXISTS clients (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    -- Public short identifier (shareable address for messages)
    identifier VARCHAR(64) UNIQUE DEFAULT NULL,
    
    -- ONLY store hash for lookup (NEVER raw identifier)
    -- Client-side generated: hash(public_key) or hash(device_fingerprint)
    client_identifier_hash VARCHAR(64) NOT NULL UNIQUE, -- SHA-256 hash
    
    -- Optional: Public key for signature verification (E2EE/authentication)
    public_key TEXT, -- Ed25519, secp256k1, or P-256 public key (flexible format)

    -- Ephemeral session management (30-day rolling sessions)
    session_token_hash VARCHAR(64), -- SHA-256 hash of session token
    session_expires_at TIMESTAMPTZ,
    
    -- Privacy & security settings
    allow_anonymous_messages BOOLEAN NOT NULL DEFAULT TRUE,
    require_proof_verification BOOLEAN NOT NULL DEFAULT FALSE, -- Require signature verification
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps (tracking without PII)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ, -- Track activity without revealing identity
    last_message_at TIMESTAMPTZ,

    -- Multi-tenancy support (optional linking)
    developer_id UUID REFERENCES users(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    
    -- Minimal, encrypted metadata (device info, preferences, etc.)
    -- NEVER store: names, emails, phone numbers, addresses, or any PII
    -- Safe to store: device type, app version, locale preferences, encryption keys
    metadata JSONB
);

-- ============================================================================
-- INDEXES
-- ============================================================================

-- Public identifier lookup (for message routing)
CREATE INDEX idx_clients_identifier ON clients(identifier);

-- Primary lookup index (most frequent query)
CREATE INDEX idx_clients_identifier_hash ON clients(client_identifier_hash);

-- Session validation index
CREATE INDEX idx_clients_session_token ON clients(session_token_hash) 
    WHERE session_token_hash IS NOT NULL;

-- Session expiry cleanup index
CREATE INDEX idx_clients_session_expiry ON clients(session_expires_at) 
    WHERE session_expires_at IS NOT NULL;

-- Activity tracking
CREATE INDEX idx_clients_last_seen ON clients(last_seen_at DESC NULLS LAST);

CREATE INDEX idx_clients_last_message ON clients(last_message_at DESC NULLS LAST);

-- Multi-tenancy queries
CREATE INDEX idx_clients_dev_api ON clients(developer_id, api_key_id);

-- Active clients filter
CREATE INDEX idx_clients_active ON clients(is_active) 
    WHERE is_active = true;

-- API key performance
CREATE INDEX idx_clients_api_key ON clients(api_key_id) 
    WHERE api_key_id IS NOT NULL;

CREATE INDEX idx_clients_active_dev ON clients(developer_id) 
    WHERE is_active = true;



-- ============================================================================
-- UPDATED MESSAGES TABLE (if not already present)
-- ============================================================================

-- Add client foreign keys to messages table
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'messages' AND column_name = 'sender_client_id'
    ) THEN
        ALTER TABLE messages 
        ADD COLUMN sender_client_id UUID REFERENCES clients(id) ON DELETE CASCADE;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'messages' AND column_name = 'recipient_client_id'
    ) THEN
        ALTER TABLE messages 
        ADD COLUMN recipient_client_id UUID REFERENCES clients(id) ON DELETE CASCADE;
    END IF;
END $$;

-- Message conversation indexes
CREATE INDEX IF NOT EXISTS idx_messages_sender_client ON messages(sender_client_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_recipient_client ON messages(recipient_client_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(sender_client_id, recipient_client_id, created_at DESC);
-- Composite index for efficient conversation queries
CREATE INDEX IF NOT EXISTS idx_messages_conversation_lookup ON messages(sender_client_id, recipient_client_id, created_at DESC)
    WHERE sender_client_id IS NOT NULL AND recipient_client_id IS NOT NULL;

-- ============================================================================
-- TRIGGERS
-- ============================================================================

-- Auto-update updated_at timestamp
CREATE OR REPLACE FUNCTION update_clients_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_clients_updated_at
    BEFORE UPDATE ON clients
    FOR EACH ROW
    EXECUTE FUNCTION update_clients_updated_at();

-- Update last_message_at for both sender and recipient
CREATE OR REPLACE FUNCTION update_client_last_message()
RETURNS TRIGGER AS $$
BEGIN
    -- Update both sender and recipient without revealing correlation
    IF NEW.sender_client_id IS NOT NULL THEN
        UPDATE clients 
        SET last_message_at = NEW.created_at
        WHERE id = NEW.sender_client_id;
    END IF;
    
    IF NEW.recipient_client_id IS NOT NULL THEN
        UPDATE clients 
        SET last_message_at = NEW.created_at
        WHERE id = NEW.recipient_client_id;
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_client_last_message
    AFTER INSERT ON messages
    FOR EACH ROW
    EXECUTE FUNCTION update_client_last_message();

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Get or create client by identifier hash (idempotent registration)
CREATE OR REPLACE FUNCTION get_or_create_client(
    p_identifier_hash VARCHAR(64),
    p_public_key TEXT DEFAULT NULL,
    p_developer_id UUID DEFAULT NULL,
    p_api_key_id UUID DEFAULT NULL,
    p_metadata JSONB DEFAULT NULL
)
RETURNS TABLE(client_id UUID, is_new BOOLEAN) AS $$
DECLARE
    v_client_id UUID;
    v_is_new BOOLEAN;
BEGIN
    -- Try to find existing client
    SELECT id INTO v_client_id
    FROM clients
    WHERE client_identifier_hash = p_identifier_hash;
    
    IF v_client_id IS NULL THEN
        -- Create new client
        INSERT INTO clients (
            client_identifier_hash,
            public_key,
            developer_id,
            api_key_id,
            metadata,
            last_seen_at
        )
        VALUES (
            p_identifier_hash,
            p_public_key,
            p_developer_id,
            p_api_key_id,
            p_metadata,
            NOW()
        )
        RETURNING id INTO v_client_id;
        
        v_is_new := TRUE;
    ELSE
        -- Update last_seen for existing client
        UPDATE clients 
        SET last_seen_at = NOW()
        WHERE id = v_client_id;
        
        v_is_new := FALSE;
    END IF;
    
    RETURN QUERY SELECT v_client_id, v_is_new;
END;
$$ LANGUAGE plpgsql;

-- Verify and refresh session
CREATE OR REPLACE FUNCTION verify_client_session(
    p_session_token_hash VARCHAR(64)
)
RETURNS TABLE(
    client_id UUID,
    is_valid BOOLEAN,
    expires_at TIMESTAMPTZ
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        c.id,
        (c.session_expires_at > NOW() AND c.is_active) as is_valid,
        c.session_expires_at
    FROM clients c
    WHERE c.session_token_hash = p_session_token_hash;
END;
$$ LANGUAGE plpgsql;

-- Clean up expired sessions (run as scheduled job)
CREATE OR REPLACE FUNCTION cleanup_expired_sessions()
RETURNS INTEGER AS $$
DECLARE
    v_deleted_count INTEGER;
BEGIN
    UPDATE clients
    SET 
        session_token_hash = NULL,
        session_expires_at = NULL
    WHERE session_expires_at < NOW() - INTERVAL '7 days';
    
    GET DIAGNOSTICS v_deleted_count = ROW_COUNT;
    RETURN v_deleted_count;
END;
$$ LANGUAGE plpgsql;

-- Deactivate inactive clients (optional cleanup)
CREATE OR REPLACE FUNCTION deactivate_inactive_clients(
    p_inactive_days INTEGER DEFAULT 90
)
RETURNS INTEGER AS $$
DECLARE
    v_deactivated_count INTEGER;
BEGIN
    UPDATE clients
    SET is_active = FALSE
    WHERE 
        last_seen_at < NOW() - (p_inactive_days || ' days')::INTERVAL
        AND is_active = TRUE;
    
    GET DIAGNOSTICS v_deactivated_count = ROW_COUNT;
    RETURN v_deactivated_count;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- VIEWS (Optional: for analytics without exposing PII)
-- ============================================================================

-- Active clients summary (privacy-preserving analytics)
CREATE OR REPLACE VIEW active_clients_summary AS
SELECT 
    DATE_TRUNC('day', created_at) as registration_date,
    COUNT(*) as total_registrations,
    COUNT(*) FILTER (WHERE last_seen_at > NOW() - INTERVAL '1 day') as active_1d,
    COUNT(*) FILTER (WHERE last_seen_at > NOW() - INTERVAL '7 days') as active_7d,
    COUNT(*) FILTER (WHERE last_seen_at > NOW() - INTERVAL '30 days') as active_30d,
    developer_id,
    api_key_id
FROM clients
WHERE is_active = TRUE
GROUP BY DATE_TRUNC('day', created_at), developer_id, api_key_id;

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON TABLE clients IS 'Anonymous ephemeral identity table - Zero personal information stored. 
Only cryptographic hashes and optional public keys. True privacy-first design.';
COMMENT ON COLUMN clients.client_identifier_hash IS 'SHA-256 hash of client identifier (public key or device fingerprint). 
MUST be computed CLIENT-SIDE. Server never sees plaintext.';
COMMENT ON COLUMN clients.public_key IS 'Optional public key for signature verification and E2EE. 
Format-agnostic: supports Ed25519, secp256k1, P-256, etc.';
COMMENT ON COLUMN clients.session_token_hash IS 'SHA-256 hash of session token. Enables stateless authentication.';
COMMENT ON COLUMN clients.metadata IS 'Encrypted metadata storage. NEVER store PII. 
Safe: device type, app version, locale, preferences. 
Forbidden: names, emails, phone numbers, addresses.';
COMMENT ON COLUMN clients.last_seen_at IS 'Privacy-preserving activity tracking. No correlation with identity.';
COMMENT ON FUNCTION get_or_create_client IS 'Idempotent client registration. Returns existing client or creates new one.';
COMMENT ON FUNCTION cleanup_expired_sessions IS 'Scheduled cleanup job. Remove sessions expired for 7+ days.';
COMMENT ON FUNCTION deactivate_inactive_clients IS 'Optional GDPR compliance. Deactivate clients inactive for N days.';

-- ============================================================================
-- SCHEDULED CLEANUP (PostgreSQL pg_cron extension)
-- ============================================================================

-- If you have pg_cron installed:
-- SELECT cron.schedule('cleanup-expired-sessions', '0 2 * * *', 
--   'SELECT cleanup_expired_sessions();');

-- SELECT cron.schedule('deactivate-inactive-clients', '0 3 * * 0', 
--   'SELECT deactivate_inactive_clients(90);');