-- ENUM for device status
CREATE TYPE iot_device_status AS ENUM ('active', 'revoked', 'suspended', 'decommissioned');

-- Main IoT devices table
CREATE TABLE iot_devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Device Identity
    device_cn TEXT NOT NULL,
    secure_element_id TEXT,
    certificate_hash TEXT NOT NULL,
    public_key_der BYTEA NOT NULL,  -- Changed from TEXT to BYTEA (binary data)
    
    -- Device Metadata
    manufacturer TEXT,
    model TEXT,
    hardware_revision TEXT,
    firmware_version TEXT,
    
    -- Status & Lifecycle
    status iot_device_status NOT NULL DEFAULT 'active',
    
    -- Timestamps
    issued_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ,
    last_attested_at TIMESTAMPTZ,
    last_firmware_update_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    
    -- Additional metadata (flexible JSON storage)
    metadata JSONB,
    
    -- Constraints
    CONSTRAINT unique_device_per_app UNIQUE (application_id, device_cn),
    CONSTRAINT unique_secure_element_per_app UNIQUE (application_id, secure_element_id),
    CONSTRAINT valid_revoked_at CHECK (
        (status = 'revoked' AND revoked_at IS NOT NULL) OR 
        (status != 'revoked' AND revoked_at IS NULL)
    )
);

-- Indexes for iot_devices
CREATE INDEX idx_iot_devices_app_status ON iot_devices(application_id, status);
CREATE INDEX idx_iot_devices_app_id ON iot_devices(application_id);
CREATE INDEX idx_iot_devices_cert_hash ON iot_devices(certificate_hash);
CREATE INDEX idx_iot_devices_secure_element_id ON iot_devices(secure_element_id) 
    WHERE secure_element_id IS NOT NULL;
CREATE INDEX idx_iot_devices_user_id ON iot_devices(user_id);
CREATE INDEX idx_iot_devices_last_seen ON iot_devices(last_seen) 
    WHERE last_seen IS NOT NULL;

-- Revocations table (audit trail)
CREATE TABLE iot_device_revocations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    device_id UUID NOT NULL REFERENCES iot_devices(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Revocation details
    device_cn TEXT NOT NULL,
    device_certificate_hash TEXT NOT NULL,
    secure_element_id TEXT,
    
    -- Revocation metadata
    reason TEXT NOT NULL,
    revoked_by UUID NOT NULL REFERENCES users(id),
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Additional context
    metadata JSONB,
    
    -- Constraints
    CONSTRAINT unique_revocation_per_cert UNIQUE (application_id, device_certificate_hash)
);

-- Indexes for iot_device_revocations
CREATE INDEX idx_iot_revocation_device_id ON iot_device_revocations(device_id);
CREATE INDEX idx_iot_revocation_cert_hash ON iot_device_revocations(device_certificate_hash);
CREATE INDEX idx_iot_revocation_app_id ON iot_device_revocations(application_id);
CREATE INDEX idx_iot_revocation_secure_element ON iot_device_revocations(secure_element_id)
    WHERE secure_element_id IS NOT NULL;
CREATE INDEX idx_iot_revocation_revoked_at ON iot_device_revocations(revoked_at);

-- Secure element change audit table (optional but recommended)
CREATE TABLE iot_device_se_changes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id UUID NOT NULL REFERENCES iot_devices(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    
    -- Change details
    old_secure_element_id TEXT,
    new_secure_element_id TEXT,
    
    -- Audit info
    changed_by UUID NOT NULL REFERENCES users(id),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reason TEXT NOT NULL,
    
    -- Additional context
    metadata JSONB
);

-- Index for SE change audit
CREATE INDEX idx_iot_se_changes_device_id ON iot_device_se_changes(device_id);
CREATE INDEX idx_iot_se_changes_changed_at ON iot_device_se_changes(changed_at);

-- Attestation logs table (for compliance and debugging)
CREATE TABLE iot_attestation_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id UUID NOT NULL REFERENCES iot_devices(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    
    -- Attestation details
    device_cn TEXT NOT NULL,
    secure_element_id TEXT,
    challenge TEXT NOT NULL,
    challenge_signature TEXT NOT NULL,
    
    -- Result
    result TEXT NOT NULL, -- 'success', 'failed'
    verdict TEXT,
    error_code TEXT,
    error_message TEXT,
    warnings JSONB,
    
    -- Metadata
    firmware_version TEXT,
    certificate_hash TEXT,
    
    -- Timestamp
    attested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Additional context
    metadata JSONB
);

-- Indexes for attestation logs
CREATE INDEX idx_iot_attestation_device_id ON iot_attestation_logs(device_id);
CREATE INDEX idx_iot_attestation_app_id ON iot_attestation_logs(application_id);
CREATE INDEX idx_iot_attestation_result ON iot_attestation_logs(result, attested_at);
CREATE INDEX idx_iot_attestation_timestamp ON iot_attestation_logs(attested_at);

-- Function to auto-update last_attested_at on successful attestation
CREATE OR REPLACE FUNCTION update_device_last_attested()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.result = 'success' THEN
        UPDATE iot_devices 
        SET 
            last_attested_at = NEW.attested_at,
            last_seen = NEW.attested_at
        WHERE id = NEW.device_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_last_attested
    AFTER INSERT ON iot_attestation_logs
    FOR EACH ROW
    EXECUTE FUNCTION update_device_last_attested();

-- Function to auto-update revoked_at when status changes to 'revoked'
CREATE OR REPLACE FUNCTION set_revoked_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status = 'revoked' AND OLD.status != 'revoked' THEN
        NEW.revoked_at = NOW();
    ELSIF NEW.status != 'revoked' THEN
        NEW.revoked_at = NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_set_revoked_timestamp
    BEFORE UPDATE ON iot_devices
    FOR EACH ROW
    WHEN (OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION set_revoked_timestamp();