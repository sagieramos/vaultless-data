-- 1. Update content_size_bytes to BIGINT
ALTER TABLE messages 
ALTER COLUMN content_size_bytes TYPE BIGINT;

-- 2. Create DLQ table
CREATE TABLE IF NOT EXISTS message_dlq (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    msg_id UUID NOT NULL,
    reason TEXT NOT NULL,
    retry_count INTEGER NOT NULL DEFAULT 0,
    original_data TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ
);

-- 3. Add indexes
CREATE INDEX idx_dlq_created ON message_dlq(created_at);
CREATE INDEX idx_dlq_msg_id ON message_dlq(msg_id);
CREATE INDEX idx_dlq_unprocessed ON message_dlq(created_at) 
    WHERE processed_at IS NULL;

-- 4. Add message indexes for performance
CREATE INDEX IF NOT EXISTS idx_messages_recipient_undelivered 
    ON messages(recipient_client_id, is_delivered, created_at)
    WHERE is_delivered = false;

CREATE INDEX IF NOT EXISTS idx_messages_delivered_at 
    ON messages(delivered_at)
    WHERE delivered_at IS NOT NULL;