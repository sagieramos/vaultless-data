-- migrations/20241022000000_add_clients_table_zero_knowledge.sql

-- ============================================================================
-- CLIENTS TABLE (Zero-Knowledge)
-- Represents message participants (senders/receivers with Zero-Knowledge)
-- ============================================================================

CREATE TABLE clients (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- ONLY store hash for lookup (NEVER raw identifier)
    client_identifier_hash VARCHAR(64) NOT NULL UNIQUE, -- SHA-256 hash
    
    -- Public key for E2EE (only public info we store)
    public_key VARCHAR(64), -- Ed25519 public key
    
    -- Privacy settings
    allow_anonymous_messages BOOLEAN NOT NULL DEFAULT TRUE,
    require_proof_verification BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_message_at TIMESTAMPTZ,
    
    -- Minimal metadata (NO personal info)
    metadata JSONB,
    
    -- Unique per user
    UNIQUE(user_id, client_identifier_hash)
);

CREATE INDEX idx_clients_user_id ON clients(user_id);
CREATE INDEX idx_clients_hash ON clients(client_identifier_hash);
CREATE INDEX idx_clients_last_message ON clients(last_message_at DESC NULLS LAST);

-- ============================================================================
-- UPDATED MESSAGES TABLE
-- ============================================================================

ALTER TABLE messages 
ADD COLUMN sender_client_id UUID REFERENCES clients(id) ON DELETE CASCADE,
ADD COLUMN recipient_client_id UUID REFERENCES clients(id) ON DELETE CASCADE;

-- Indexes
CREATE INDEX idx_messages_sender_client ON messages(sender_client_id, created_at DESC);
CREATE INDEX idx_messages_recipient_client ON messages(recipient_client_id, created_at DESC);
CREATE INDEX idx_messages_conversation ON messages(sender_client_id, recipient_client_id, created_at DESC);

-- ============================================================================
-- TRIGGERS
-- ============================================================================

-- Update last_message_at without revealing who
CREATE OR REPLACE FUNCTION update_client_last_message()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE clients 
    SET last_message_at = NEW.created_at
    WHERE id IN (NEW.sender_client_id, NEW.recipient_client_id);
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

-- Get or create client by hash (client computes hash on their side)
CREATE OR REPLACE FUNCTION get_or_create_client_by_hash(
    p_user_id UUID,
    p_identifier_hash VARCHAR(64),
    p_client_type VARCHAR DEFAULT 'email',
    p_public_key TEXT DEFAULT NULL
)
RETURNS UUID AS $$
DECLARE
    v_client_id UUID;
BEGIN
    -- Try to find existing
    SELECT id INTO v_client_id
    FROM clients
    WHERE user_id = p_user_id 
        AND client_identifier_hash = p_identifier_hash;
    
    -- Create if doesn't exist
    IF v_client_id IS NULL THEN
        INSERT INTO clients (user_id, client_identifier_hash, client_type, public_key)
        VALUES (p_user_id, p_identifier_hash, p_client_type, p_public_key)
        RETURNING id INTO v_client_id;
    END IF;
    
    RETURN v_client_id;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON TABLE clients IS 'Zero-knowledge client table - ONLY hashes stored, never plaintext';
COMMENT ON COLUMN clients.client_identifier_hash IS 'SHA-256 hash computed CLIENT-SIDE';
COMMENT ON COLUMN clients.public_key IS 'Ed25519 public key (not private - safe to store)';
COMMENT ON COLUMN clients.metadata IS 'NEVER store personal info here - encryption keys only';