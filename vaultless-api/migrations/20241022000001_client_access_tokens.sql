-- ============================================================================
-- CLIENT ACCESS TOKENS
-- Short-lived tokens for message retrieval (rotates frequently)
-- ============================================================================

CREATE TABLE client_access_tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    client_id UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    
    -- Token hash (client stores full token, we store hash)
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    
    -- Token metadata
    token_type VARCHAR(50) NOT NULL DEFAULT 'message_retrieval', -- 'message_retrieval', 'message_send'
    scopes TEXT[], -- ['read', 'write', 'delete']
    
    -- Expiry
    expires_at TIMESTAMPTZ NOT NULL,
    
    -- Usage tracking
    last_used_at TIMESTAMPTZ,
    use_count INTEGER NOT NULL DEFAULT 0,
    
    -- Security
    max_uses INTEGER, -- NULL = unlimited
    ip_whitelist TEXT[], -- Optional IP restriction
    
    -- Status
    is_revoked BOOLEAN NOT NULL DEFAULT FALSE,
    revoked_at TIMESTAMPTZ,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT valid_expiry CHECK (expires_at > created_at),
    CONSTRAINT valid_uses CHECK (max_uses IS NULL OR use_count <= max_uses)
);

CREATE INDEX idx_client_tokens_hash ON client_access_tokens(token_hash);
CREATE INDEX idx_client_tokens_client ON client_access_tokens(client_id);
CREATE INDEX idx_client_tokens_expires ON client_access_tokens(expires_at);
CREATE INDEX idx_client_tokens_active ON client_access_tokens(is_revoked, expires_at) 
    WHERE is_revoked = FALSE;

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Create access token for client
CREATE OR REPLACE FUNCTION create_client_access_token(
    p_client_id UUID,
    p_token_hash VARCHAR(64),
    p_expires_in_hours INTEGER DEFAULT 24
)
RETURNS UUID AS $$
DECLARE
    v_token_id UUID;
BEGIN
    INSERT INTO client_access_tokens (
        client_id, 
        token_hash, 
        expires_at
    )
    VALUES (
        p_client_id,
        p_token_hash,
        NOW() + (p_expires_in_hours || ' hours')::INTERVAL
    )
    RETURNING id INTO v_token_id;
    
    RETURN v_token_id;
END;
$$ LANGUAGE plpgsql;

-- Validate and get client from token
CREATE OR REPLACE FUNCTION validate_client_token(
    p_token_hash VARCHAR(64)
)
RETURNS UUID AS $$
DECLARE
    v_client_id UUID;
BEGIN
    -- Update usage and return client_id
    UPDATE client_access_tokens
    SET last_used_at = NOW(),
        use_count = use_count + 1
    WHERE token_hash = p_token_hash
        AND is_revoked = FALSE
        AND expires_at > NOW()
        AND (max_uses IS NULL OR use_count < max_uses)
    RETURNING client_id INTO v_client_id;
    
    RETURN v_client_id;
END;
$$ LANGUAGE plpgsql;

-- Revoke all tokens for a client
CREATE OR REPLACE FUNCTION revoke_client_tokens(p_client_id UUID)
RETURNS INTEGER AS $$
DECLARE
    v_count INTEGER;
BEGIN
    UPDATE client_access_tokens
    SET is_revoked = TRUE,
        revoked_at = NOW()
    WHERE client_id = p_client_id
        AND is_revoked = FALSE;
    
    GET DIAGNOSTICS v_count = ROW_COUNT;
    RETURN v_count;
END;
$$ LANGUAGE plpgsql;

-- Cleanup expired tokens
CREATE OR REPLACE FUNCTION cleanup_expired_client_tokens()
RETURNS INTEGER AS $$
DECLARE
    v_count INTEGER;
BEGIN
    DELETE FROM client_access_tokens
    WHERE expires_at < NOW() - INTERVAL '7 days'; -- Keep 7 days for audit
    
    GET DIAGNOSTICS v_count = ROW_COUNT;
    RETURN v_count;
END;
$$ LANGUAGE plpgsql;