-- Add migration script here
CREATE TABLE message_dlq (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    msg_id UUID NOT NULL,
    reason TEXT NOT NULL,
    retry_count INTEGER NOT NULL,
    original_data TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ
);

CREATE INDEX idx_dlq_created ON message_dlq(created_at);
CREATE INDEX idx_dlq_msg_id ON message_dlq(msg_id);
CREATE INDEX idx_dlq_unprocessed ON message_dlq(processed_at) 
WHERE processed_at IS NULL;