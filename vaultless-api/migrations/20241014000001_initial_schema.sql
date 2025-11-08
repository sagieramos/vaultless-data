CREATE EXTENSION IF NOT EXISTS "timescaledb";

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
COMMENT ON TABLE user_sessions IS 'Session audit trail - primary storage is Dragonfly/Redis';
COMMENT ON COLUMN user_sessions.access_token_hash IS 'SHA-256 hash of access token for lookup';
COMMENT ON TABLE refresh_tokens IS 'Long-lived refresh tokens (30-90 days) with rotation';
COMMENT ON TABLE oauth_scopes IS 'OAuth 2.0 style permission scopes';
COMMENT ON TABLE login_attempts IS 'Security audit trail for login attempts';

COMMENT ON COLUMN refresh_tokens.token_family IS 'Detects token theft - if old token reused, revoke entire family';
COMMENT ON COLUMN user_sessions.scope IS 'OAuth scopes for this session (space-separated)';

-- ============================================================================
-- API KEYS TABLE
-- Stores authentication keys and subscription tiers
-- ============================================================================
CREATE TYPE subscription_tier AS ENUM ('free', 'starter', 'pro', 'enterprise');

CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    
    -- Core link
    user_id UUID REFERENCES users(id) ON DELETE CASCADE, -- Owner of the API key
    
    -- API key data
    key_hash VARCHAR(64) NOT NULL UNIQUE,               -- SHA-256 hash of the API key
    key_prefix VARCHAR(32) NOT NULL,                    -- First 8 chars for identification (vlt_xxxxx...)
    
    -- Subscription & Billing
    tier subscription_tier NOT NULL DEFAULT 'free',
    monthly_message_quota INTEGER NOT NULL DEFAULT 1000,
    message_retention_seconds INTEGER NOT NULL DEFAULT 604800, -- 7 days
    
    -- Metadata
    description TEXT,                                   -- User-friendly description
    scopes TEXT,                                        -- Space-separated OAuth scopes
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,                             -- NULL = no expiration
    last_used_at TIMESTAMPTZ,
    
    -- Rate limiting
    rate_limit_per_minute INTEGER NOT NULL DEFAULT 60,
    
    -- Constraints
    CONSTRAINT valid_quota CHECK (monthly_message_quota > 0),
    CONSTRAINT valid_retention CHECK (message_retention_seconds > 0)
);

-- Indexes
CREATE INDEX idx_api_keys_user_id ON api_keys(user_id);
CREATE INDEX idx_api_keys_active ON api_keys(is_active) WHERE is_active = true;
CREATE INDEX idx_api_keys_tier ON api_keys(tier);

-- ============================================================================
-- MESSAGES TABLE
-- Stores encrypted message payloads (zero-knowledge)
-- ============================================================================
CREATE TABLE messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    
    -- Encrypted Payload
    ciphertext TEXT NOT NULL, -- Base64-encoded AES-256-GCM encrypted data
    nonce UUID NOT NULL,
    
    -- Message Metadata (encrypted or public)
    content_type VARCHAR(100) DEFAULT 'application/octet-stream',
    content_size_bytes INTEGER NOT NULL,
    
    -- Authentication
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    accessed_at TIMESTAMPTZ, -- NULL = never accessed
    access_count INTEGER NOT NULL DEFAULT 0,
    
    -- Delivery tracking
    is_delivered BOOLEAN NOT NULL DEFAULT false,
    delivered_at TIMESTAMPTZ,
    
    -- Optional features
    max_access_count INTEGER, -- NULL = unlimited
    require_proof_verification BOOLEAN NOT NULL DEFAULT false,
    
    -- Indexes
    CONSTRAINT valid_content_size CHECK (content_size_bytes > 0),
    CONSTRAINT valid_access_count CHECK (access_count >= 0),
    CONSTRAINT valid_max_access CHECK (max_access_count IS NULL OR max_access_count > 0)
);

CREATE INDEX idx_messages_api_key ON messages(api_key_id);
CREATE INDEX idx_messages_expires ON messages(expires_at);
CREATE INDEX idx_messages_created ON messages(created_at DESC);
CREATE INDEX idx_messages_delivered ON messages(is_delivered) WHERE is_delivered = false;

-- ============================================================================
-- MESSAGE PROOFS TABLE
-- Cryptographic verification data (Ed25519 signatures + SHA-256 hashes)
-- ============================================================================
CREATE TABLE message_proofs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    
    -- Cryptographic Proofs
    content_hash VARCHAR(64) NOT NULL, -- SHA-256 hash of original plaintext
    signature TEXT NOT NULL, -- Ed25519 signature (base64 encoded)
    public_key TEXT NOT NULL, -- Ed25519 public key (base64 encoded)
    
    -- Verification metadata
    algorithm VARCHAR(50) NOT NULL DEFAULT 'Ed25519',
    hash_algorithm VARCHAR(50) NOT NULL DEFAULT 'SHA-256',
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    verified_at TIMESTAMPTZ, -- NULL = not yet verified
    verification_count INTEGER NOT NULL DEFAULT 0,
    
    -- Additional context
    proof_metadata JSONB, -- Extensible field for custom proof data
    
    CONSTRAINT valid_verification_count CHECK (verification_count >= 0)
);

CREATE INDEX idx_proofs_message ON message_proofs(message_id);
CREATE INDEX idx_proofs_content_hash ON message_proofs(content_hash);
CREATE INDEX idx_proofs_verified ON message_proofs(verified_at);

-- ============================================================================
-- USAGE METRICS TABLE
-- Track API usage for billing and analytics
-- ============================================================================
CREATE TABLE usage_metrics (
    id UUID DEFAULT uuid_generate_v4(),
    api_key_id UUID NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    
    -- Time window
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    
    -- Usage counters
    messages_sent BIGINT NOT NULL DEFAULT 0,
    messages_received BIGINT NOT NULL DEFAULT 0,
    proofs_verified BIGINT NOT NULL DEFAULT 0,
    total_bytes_stored BIGINT NOT NULL DEFAULT 0,
    total_bytes_sent BIGINT NOT NULL DEFAULT 0,
    total_bytes_received BIGINT NOT NULL DEFAULT 0,
    
    -- Rate limiting violations
    rate_limit_hits INTEGER NOT NULL DEFAULT 0,
    
    -- Cost tracking (for billing)
    estimated_cost_cents INTEGER,
    
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT valid_period CHECK (period_end > period_start),
    CONSTRAINT valid_counters CHECK (
        messages_sent >= 0 AND 
        messages_received >= 0 AND 
        proofs_verified >= 0 AND
        total_bytes_stored >= 0 AND
        total_bytes_sent >= 0 AND
        total_bytes_received >= 0 AND
        rate_limit_hits >= 0
    )
);

CREATE INDEX idx_usage_metrics_id ON usage_metrics(id);

-- Turn it into a hypertable (time-series)
SELECT create_hypertable('usage_metrics', 'period_start', if_not_exists => TRUE);

CREATE INDEX idx_usage_api_key ON usage_metrics(api_key_id);
CREATE INDEX idx_usage_period ON usage_metrics(period_start, period_end);
CREATE INDEX idx_usage_created ON usage_metrics(created_at DESC);
CREATE INDEX idx_usage_bytes_sent ON usage_metrics (total_bytes_sent ASC NULLS LAST);
CREATE INDEX idx_usage_bytes_received ON usage_metrics (total_bytes_received ASC NULLS LAST);

-- Unique constraint: one metric record per API key per period
-- We enforce this at application level with period_start rounded to the hour
CREATE UNIQUE INDEX idx_usage_unique_period ON usage_metrics(api_key_id, period_start);

-- ============================================================================
-- TIMESCALE: compression + retention
-- ============================================================================
-- Enable compression and segment by api_key_id (reduces storage for older data)
ALTER TABLE usage_metrics SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'api_key_id'
);

-- Automatically compress chunks older than 7 days
SELECT add_compression_policy('usage_metrics', INTERVAL '7 days');

-- Automatically drop (retention) raw hourly data older than 90 days
SELECT add_retention_policy('usage_metrics', INTERVAL '90 days');

-- ============================================================================
-- CONTINUOUS AGGREGATE: daily summary (fast daily rollups)
-- ============================================================================

-- Create continuous aggregate (materialized view) for daily totals
CREATE MATERIALIZED VIEW IF NOT EXISTS usage_metrics_daily
WITH (timescaledb.continuous) AS
SELECT
    api_key_id,
    time_bucket(INTERVAL '1 day', period_start) AS day,
    COALESCE(SUM(messages_sent), 0)::BIGINT        AS total_messages_sent,
    COALESCE(SUM(messages_received), 0)::BIGINT    AS total_messages_received,
    COALESCE(SUM(proofs_verified), 0)::BIGINT      AS total_proofs_verified,
    COALESCE(SUM(total_bytes_stored), 0)::BIGINT   AS total_bytes_stored,
    COALESCE(SUM(rate_limit_hits), 0)::BIGINT      AS total_rate_limit_hits,
    COALESCE(SUM(COALESCE(estimated_cost_cents, 0)), 0)::BIGINT AS total_estimated_cost_cents
FROM usage_metrics
GROUP BY api_key_id, time_bucket(INTERVAL '1 day', period_start)
WITH NO DATA;


-- Schedule automatic refresh policy (keeps data up-to-date; adjust offsets as desired)
SELECT add_continuous_aggregate_policy(
    'usage_metrics_daily',
    start_offset => INTERVAL '2 day',
    end_offset   => INTERVAL '0 hour',
    schedule_interval => INTERVAL '1 hour'
);

-- 🚀 Composite index for high-performance time-series lookups per API key
CREATE INDEX IF NOT EXISTS idx_usage_metrics_daily_api_key_day ON usage_metrics_daily (api_key_id, day DESC);

-- ============================================================================
-- CONTINUOUS AGGREGATE: weekly summary (long-term rollups)
-- ============================================================================

CREATE MATERIALIZED VIEW IF NOT EXISTS usage_metrics_weekly
WITH (timescaledb.continuous) AS
SELECT
    api_key_id,
    time_bucket(INTERVAL '7 days', period_start) AS week_start,
    COALESCE(SUM(messages_sent), 0)::BIGINT        AS total_messages_sent,
    COALESCE(SUM(messages_received), 0)::BIGINT    AS total_messages_received,
    COALESCE(SUM(proofs_verified), 0)::BIGINT      AS total_proofs_verified,
    COALESCE(SUM(total_bytes_stored), 0)::BIGINT   AS total_bytes_stored,
    COALESCE(SUM(rate_limit_hits), 0)::BIGINT      AS total_rate_limit_hits,
    COALESCE(SUM(COALESCE(estimated_cost_cents, 0)), 0)::BIGINT AS total_estimated_cost_cents
FROM usage_metrics
GROUP BY api_key_id, time_bucket(INTERVAL '7 days', period_start)
WITH NO DATA;

SELECT add_continuous_aggregate_policy(
    'usage_metrics_weekly',
    start_offset => INTERVAL '180 days',
    end_offset   => INTERVAL '1 day',
    schedule_interval => INTERVAL '1 day'
);

-- Useful indexes on the weekly aggregate
CREATE INDEX IF NOT EXISTS idx_usage_metrics_weekly_api_key_week_start ON usage_metrics_weekly (api_key_id, week_start DESC);

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Function to clean up expired messages
CREATE OR REPLACE FUNCTION cleanup_expired_messages()
RETURNS INTEGER AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM messages WHERE expires_at < NOW();
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$ LANGUAGE plpgsql;

-- Function to update API key last_used_at
CREATE OR REPLACE FUNCTION update_api_key_last_used()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE api_keys 
    SET last_used_at = NOW() 
    WHERE id = NEW.api_key_id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_api_key_usage
    AFTER INSERT ON messages
    FOR EACH ROW
    EXECUTE FUNCTION update_api_key_last_used();

-- Function to increment message access count
CREATE OR REPLACE FUNCTION increment_message_access()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE messages 
    SET 
        access_count = access_count + 1,
        accessed_at = NOW(),
        is_delivered = CASE 
            WHEN max_access_count IS NOT NULL AND access_count + 1 >= max_access_count 
            THEN true 
            ELSE is_delivered 
        END
    WHERE id = NEW.message_id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Note: This trigger would be activated by a "message_accesses" event table if needed

-- ============================================================================
-- SEED DATA (Development Only)
-- ============================================================================

-- Create a development API key (hash of "dev_test_key_12345")
/* INSERT INTO api_keys (
    key_hash,
    key_prefix,
    tier,
    monthly_message_quota,
    message_retention_seconds,
    owner_email,
    owner_name,
    rate_limit_per_minute,
) VALUES (
    'a665a45920422f9d417e4867efdc4fb8a04a1f3fff1fa07e998e86f7f7a27ae3', -- SHA-256 of "dev_test_key_12345"
    'vlt_dev_',
    'pro',
    500000,
    7776000, -- 90 days
    'dev@vaultless.local',
    'Development User',
    1000,
    'Development API key - DO NOT USE IN PRODUCTION'
) ON CONFLICT (key_hash) DO NOTHING; */

-- ============================================================================
-- COMMENTS FOR DOCUMENTATION
-- ============================================================================

COMMENT ON TABLE api_keys IS 'Authentication keys with subscription tier and quota management';
COMMENT ON TABLE messages IS 'Encrypted message storage - backend never sees plaintext';
COMMENT ON TABLE message_proofs IS 'Cryptographic verification data using Ed25519 signatures';
COMMENT ON TABLE usage_metrics IS 'Hourly aggregated usage data for billing and analytics';

COMMENT ON COLUMN messages.ciphertext IS 'AES-256-GCM encrypted payload (base64)';
COMMENT ON COLUMN messages.nonce IS '96-bit nonce for AES-GCM (base64)';
COMMENT ON COLUMN message_proofs.content_hash IS 'SHA-256 hash of original plaintext for verification';