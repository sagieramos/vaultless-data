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
    public_key_der BYTEA NOT NULL,
    
    -- Device Metadata
    manufacturer TEXT,
    model TEXT,
    hardware_revision TEXT,
    firmware_version TEXT,
    
    -- Status & Lifecycle
    status iot_device_status NOT NULL DEFAULT 'active',
    
    -- Timestamps
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen TIMESTAMPTZ,
    
    -- Constraints
    CONSTRAINT unique_device_per_app UNIQUE (application_id, device_cn),
    CONSTRAINT unique_secure_element_per_app UNIQUE (application_id, secure_element_id)
);

-- Indexes for iot_devices
CREATE INDEX idx_iot_devices_app_status ON iot_devices(application_id, status);
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
    
    -- Revocation details
    device_cn TEXT NOT NULL,
    device_certificate_hash TEXT NOT NULL,
    reason TEXT NOT NULL,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Constraints
    CONSTRAINT unique_revocation_per_cert UNIQUE (application_id, device_certificate_hash)
);

-- Indexes for iot_device_revocations
CREATE INDEX idx_iot_revocation_device_id ON iot_device_revocations(device_id);
CREATE INDEX idx_iot_revocation_cert_hash ON iot_device_revocations(device_certificate_hash);
CREATE INDEX idx_iot_revocation_app_id ON iot_device_revocations(application_id);