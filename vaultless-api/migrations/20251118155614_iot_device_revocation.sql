-- Add migration script here
CREATE TABLE iot_device_revocations (
    id BIGSERIAL PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id),
    user_id UUID NOT NULL REFERENCES users(id),
    device_certificate_hash TEXT NOT NULL,
    device_id TEXT NOT NULL,
    revoked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reason TEXT,
    revoked_by UUID REFERENCES users(id),
    UNIQUE(application_id, device_certificate_hash)
);

-- Indexes for fast lookup
CREATE INDEX idx_iot_revocation_cert_hash ON iot_device_revocations(device_certificate_hash);
CREATE INDEX idx_iot_revocation_device_id ON iot_device_revocations(device_id);
