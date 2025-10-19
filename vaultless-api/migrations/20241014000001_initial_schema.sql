CREATE EXTENSION IF NOT EXISTS "timescaledb";

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
    key_prefix VARCHAR(8) NOT NULL,                    -- First 8 chars for identification (vlt_xxxxx...)
    
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
CREATE INDEX idx_api_keys_key_hash ON api_keys(key_hash);
CREATE INDEX idx_api_keys_active ON api_keys(is_active) WHERE is_active = true;
CREATE INDEX idx_api_keys_tier ON api_keys(tier);

-- ============================================================================
-- MESSAGES TABLE
-- Stores encrypted message payloads (zero-knowledge)
-- ============================================================================
CREATE TABLE messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    
    -- Routing
    recipient_id VARCHAR(255) NOT NULL, -- User-defined recipient identifier (can be hashed)
    
    -- Encrypted Payload
    ciphertext TEXT NOT NULL, -- Base64-encoded AES-256-GCM encrypted data
    nonce VARCHAR(32) NOT NULL, -- Base64-encoded 96-bit nonce for AES-GCM
    
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

CREATE INDEX idx_messages_recipient ON messages(recipient_id);
CREATE INDEX idx_messages_api_key ON messages(api_key_id);
CREATE INDEX idx_messages_expires ON messages(expires_at);
CREATE INDEX idx_messages_created ON messages(created_at DESC);
CREATE INDEX idx_messages_delivered ON messages(is_delivered) WHERE is_delivered = false;

-- Composite index for message retrieval
CREATE INDEX idx_messages_recipient_undelivered ON messages(recipient_id, is_delivered) 
    WHERE is_delivered = false;

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
        total_bytes_stored >= 0
    )
);

CREATE INDEX idx_usage_metrics_id ON usage_metrics(id);

-- Turn it into a hypertable (time-series)
SELECT create_hypertable('usage_metrics', 'period_start', if_not_exists => TRUE);

CREATE INDEX idx_usage_api_key ON usage_metrics(api_key_id);
CREATE INDEX idx_usage_period ON usage_metrics(period_start, period_end);
CREATE INDEX idx_usage_created ON usage_metrics(created_at DESC);

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
INSERT INTO api_keys (
    key_hash,
    key_prefix,
    tier,
    monthly_message_quota,
    message_retention_seconds,
    owner_email,
    owner_name,
    rate_limit_per_minute,
    notes
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
) ON CONFLICT (key_hash) DO NOTHING;

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