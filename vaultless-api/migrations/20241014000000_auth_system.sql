-- 20241014000000_auth_system.sql
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================================================
-- USERS TABLE
-- Core identity layer
-- ============================================================================
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL, -- bcrypt/argon2
    
    -- Profile
    name VARCHAR(255),
    avatar_url TEXT,
    
    -- Verification
    email_verified BOOLEAN NOT NULL DEFAULT false,
    email_verification_token VARCHAR(255),
    email_verification_expires_at TIMESTAMPTZ,
    
    -- Password reset
    password_reset_token VARCHAR(255),
    password_reset_expires_at TIMESTAMPTZ,
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT true,
    is_admin BOOLEAN NOT NULL DEFAULT false,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_login_at TIMESTAMPTZ,
    
    -- Billing (for future Stripe)
    stripe_customer_id VARCHAR(255),
    
    -- Metadata
    metadata JSONB
);

CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_email_verified ON users(email_verified) WHERE email_verified = true;
CREATE INDEX idx_users_active ON users(is_active) WHERE is_active = true;

-- ============================================================================
-- USER SESSIONS TABLE
-- Short-lived access tokens (JWT-like but server-side)
-- ============================================================================
CREATE TABLE user_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Session tokens
    access_token VARCHAR(255) NOT NULL UNIQUE,
    access_token_hash VARCHAR(64) NOT NULL, -- SHA-256 for lookup
    
    -- OAuth-like fields
    token_type VARCHAR(50) NOT NULL DEFAULT 'Bearer',
    scope TEXT, -- Space-separated scopes
    
    -- Expiry
    expires_at TIMESTAMPTZ NOT NULL,
    
    -- Device/Client info
    user_agent TEXT,
    ip_address INET,
    device_id VARCHAR(255),
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT true,
    revoked_at TIMESTAMPTZ,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ,
    
    CONSTRAINT valid_expiry CHECK (expires_at > created_at)
);

CREATE INDEX idx_sessions_access_token_hash ON user_sessions(access_token_hash);
CREATE INDEX idx_sessions_user_id ON user_sessions(user_id);
CREATE INDEX idx_sessions_expires ON user_sessions(expires_at);
CREATE INDEX idx_sessions_active ON user_sessions(is_active, expires_at) WHERE is_active = true;

-- ============================================================================
-- REFRESH TOKENS TABLE
-- Long-lived tokens for obtaining new access tokens
-- ============================================================================
CREATE TABLE refresh_tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id UUID REFERENCES user_sessions(id) ON DELETE CASCADE,
    
    -- Token
    token_hash VARCHAR(64) NOT NULL UNIQUE, -- SHA-256 of refresh token
    
    -- Token family (for rotation detection)
    token_family UUID NOT NULL, -- All rotations share same family
    parent_token_id UUID REFERENCES refresh_tokens(id), -- For rotation chain
    
    -- Expiry
    expires_at TIMESTAMPTZ NOT NULL,
    
    -- Status
    is_used BOOLEAN NOT NULL DEFAULT false,
    used_at TIMESTAMPTZ,
    is_revoked BOOLEAN NOT NULL DEFAULT false,
    revoked_at TIMESTAMPTZ,
    revoked_reason VARCHAR(255),
    
    -- Device/Client info
    device_id VARCHAR(255),
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT valid_expiry CHECK (expires_at > created_at)
);

CREATE INDEX idx_refresh_tokens_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE INDEX idx_refresh_tokens_family ON refresh_tokens(token_family);
CREATE INDEX idx_refresh_tokens_active ON refresh_tokens(is_used, is_revoked, expires_at) 
    WHERE is_used = false AND is_revoked = false;

-- ============================================================================
-- OAUTH SCOPES TABLE (Future expansion)
-- ============================================================================
CREATE TABLE oauth_scopes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    scope VARCHAR(255) NOT NULL UNIQUE,
    description TEXT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed default scopes
INSERT INTO oauth_scopes (scope, description, is_default) VALUES
    ('messages:read', 'Read encrypted messages', true),
    ('messages:write', 'Send encrypted messages', true),
    ('messages:delete', 'Delete messages', false),
    ('keys:read', 'Read API key information', true),
    ('keys:write', 'Create and manage API keys', false),
    ('keys:delete', 'Delete API keys', false),
    ('analytics:read', 'Read usage analytics', true),
    ('admin:read', 'Read admin data', false),
    ('admin:write', 'Perform admin actions', false);

-- ============================================================================
-- LOGIN ATTEMPTS TABLE (Security)
-- Track failed login attempts for rate limiting
-- ============================================================================
CREATE TABLE login_attempts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email VARCHAR(255) NOT NULL,
    ip_address INET NOT NULL,
    success BOOLEAN NOT NULL,
    failure_reason VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_login_attempts_email ON login_attempts(email, created_at DESC);
CREATE INDEX idx_login_attempts_ip ON login_attempts(ip_address, created_at DESC);

-- Auto-cleanup old attempts (keep 30 days)
CREATE INDEX idx_login_attempts_cleanup ON login_attempts(created_at);

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Function to clean up login attempts
CREATE OR REPLACE FUNCTION cleanup_old_login_attempts(retention_days INT)
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM login_attempts
    WHERE created_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;


-- Function to clean up expired sessions
CREATE OR REPLACE FUNCTION cleanup_expired_sessions()
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM user_sessions WHERE expires_at < NOW();
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;

-- Function to clean up expired refresh tokens
CREATE OR REPLACE FUNCTION cleanup_expired_refresh_tokens()
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM refresh_tokens 
    WHERE expires_at < NOW() 
        OR (is_used = true AND created_at < NOW() - INTERVAL '7 days');
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;

-- Function to revoke all user sessions
CREATE OR REPLACE FUNCTION revoke_user_sessions(p_user_id UUID)
RETURNS INTEGER AS $$
DECLARE
    updated_count INTEGER;
BEGIN
    UPDATE user_sessions 
    SET is_active = false, revoked_at = NOW() 
    WHERE user_id = p_user_id AND is_active = true;
    
    GET DIAGNOSTICS updated_count = ROW_COUNT;
    RETURN updated_count;
END;
$$ LANGUAGE plpgsql;

-- Function to revoke refresh token family (detect token theft)
CREATE OR REPLACE FUNCTION revoke_refresh_token_family(p_token_family UUID)
RETURNS INTEGER AS $$
DECLARE
    updated_count INTEGER;
BEGIN
    UPDATE refresh_tokens 
    SET 
        is_revoked = true, 
        revoked_at = NOW(),
        revoked_reason = 'Token family compromised - possible theft detected'
    WHERE token_family = p_token_family 
        AND is_revoked = false;
    
    GET DIAGNOSTICS updated_count = ROW_COUNT;
    RETURN updated_count;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- TRIGGERS
-- ============================================================================

-- Update updated_at on users table
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON TABLE users IS 'Core user identity and authentication';
COMMENT ON TABLE user_sessions IS 'Short-lived access tokens (15-60 min)';
COMMENT ON TABLE refresh_tokens IS 'Long-lived refresh tokens (30-90 days) with rotation';
COMMENT ON TABLE oauth_scopes IS 'OAuth 2.0 style permission scopes';
COMMENT ON TABLE login_attempts IS 'Security audit trail for login attempts';

COMMENT ON COLUMN refresh_tokens.token_family IS 'Detects token theft - if old token reused, revoke entire family';
COMMENT ON COLUMN user_sessions.scope IS 'OAuth scopes for this session (space-separated)';