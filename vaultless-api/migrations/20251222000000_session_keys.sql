-- Migration: Upgrade to dual-key cryptography architecture
-- Ed25519 for signing, X25519 for key exchange
-- Adds support for ephemeral session keys and forward secrecy

-- Step 1: Add new columns to clients table
ALTER TABLE public.clients
ADD COLUMN signing_key TEXT;

-- Step 2: Migrate existing public_key data to signing_key
UPDATE public.clients SET signing_key = public_key WHERE public_key IS NOT NULL;

-- Step 3: Drop old constraints on public_key
ALTER TABLE public.clients DROP CONSTRAINT IF EXISTS clients_public_key_key;

-- Step 4: Drop the old public_key column (force re-registration approach)
ALTER TABLE public.clients DROP COLUMN public_key;

-- Step 5: Add NOT NULL constraints
ALTER TABLE public.clients ALTER COLUMN signing_key SET NOT NULL;

-- Step 6: Add unique constraints for new keys
ALTER TABLE public.clients
ADD CONSTRAINT clients_signing_key_key UNIQUE (signing_key);

-- Step 7: Create indexes for key lookups
CREATE INDEX IF NOT EXISTS idx_clients_signing_key
ON public.clients USING btree (signing_key)
WHERE signing_key IS NOT NULL;

-- Step 8: Create session_keys table for ephemeral key storage (updated)
CREATE TABLE IF NOT EXISTS public.session_keys (
    id UUID NOT NULL DEFAULT uuid_generate_v4(),
    client_id UUID NOT NULL,
    peer_client_id UUID NOT NULL,
    application_id UUID NOT NULL,

    -- Session identification
    session_id VARCHAR(64) NOT NULL,

    -- Ephemeral X25519 public key for this session
    ephemeral_public_key TEXT NOT NULL,

    -- Session metadata
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL,

    -- Algorithm version for crypto
    algorithm_version SMALLINT NOT NULL DEFAULT 1,

    -- Session state
    is_active BOOLEAN NOT NULL DEFAULT true,

    CONSTRAINT session_keys_pkey PRIMARY KEY (id),
    CONSTRAINT session_keys_client_id_fkey FOREIGN KEY (client_id)
        REFERENCES public.clients (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE CASCADE,
    CONSTRAINT session_keys_peer_client_id_fkey FOREIGN KEY (peer_client_id)
        REFERENCES public.clients (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE CASCADE,
    CONSTRAINT session_keys_application_id_fkey FOREIGN KEY (application_id)
        REFERENCES public.applications (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE CASCADE,
    CONSTRAINT valid_expiry CHECK (expires_at > created_at)
);

-- Index for application-based queries
CREATE INDEX IF NOT EXISTS idx_session_keys_application
ON public.session_keys USING btree (application_id, is_active)
WHERE is_active = true;

-- Step 8b: Add partial unique index for active sessions
CREATE UNIQUE INDEX IF NOT EXISTS session_keys_active_pair_unique
ON public.session_keys (client_id, peer_client_id)
WHERE is_active = true;

-- Indexes for session_keys
CREATE INDEX IF NOT EXISTS idx_session_keys_client
ON public.session_keys USING btree (client_id, is_active)
WHERE is_active = true;

CREATE INDEX IF NOT EXISTS idx_session_keys_peer
ON public.session_keys USING btree (peer_client_id, is_active)
WHERE is_active = true;

CREATE INDEX IF NOT EXISTS idx_session_keys_expires
ON public.session_keys USING btree (expires_at)
WHERE is_active = true;

CREATE INDEX IF NOT EXISTS idx_session_keys_session_id
ON public.session_keys USING btree (session_id);

-- Step 9: Add encryption algorithm version to messages
ALTER TABLE public.messages
ADD COLUMN encryption_algorithm VARCHAR(32) DEFAULT 'xchacha20-poly1305',
ADD COLUMN algorithm_version SMALLINT DEFAULT 1;

-- Index for algorithm filtering (useful for migrations)
CREATE INDEX IF NOT EXISTS idx_messages_algorithm
ON public.messages USING btree (encryption_algorithm, algorithm_version);

-- Step 10: Create function to cleanup expired sessions
CREATE OR REPLACE FUNCTION cleanup_expired_sessions_crypto()
RETURNS void AS $$
BEGIN
    UPDATE public.session_keys
    SET is_active = false
    WHERE expires_at < NOW() AND is_active = true;
END;
$$ LANGUAGE plpgsql;

-- Step 11: Add comment documentation
COMMENT ON COLUMN public.clients.signing_key IS 'Ed25519 public key for signature verification (authentication)';
COMMENT ON TABLE public.session_keys IS 'Ephemeral session keys for forward secrecy in client-to-client communication';
COMMENT ON COLUMN public.messages.encryption_algorithm IS 'Algorithm used: aes-256-gcm (legacy) or xchacha20-poly1305 (current)';
COMMENT ON COLUMN public.messages.algorithm_version IS 'Version number for algorithm parameters and key derivation method';
COMMENT ON FUNCTION cleanup_expired_sessions_crypto() IS 'Scheduled cleanup job. Remove sessions expired for 7+ days.';

-- Step 12: Grant permissions
ALTER TABLE public.session_keys OWNER TO vaultless;

-- Cron scheduling example (uncomment to activate)
-- SELECT cron.schedule('cleanup-expired-sessions', '0 2 * * *', 
--   'SELECT cleanup_expired_sessions_crypto();');
