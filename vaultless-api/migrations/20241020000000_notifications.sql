-- Add notification system with enums and indexes

-- Create notification type enum
CREATE TYPE notification_type AS ENUM (
    'quota_warning',
    'quota_exceeded',
    'billing_alert',
    'security_alert',
    'system_update',
    'marketing_offer',
    'api_key_expiring',
    'usage_report'
);

-- Create notification severity enum
CREATE TYPE notification_severity AS ENUM (
    'info',
    'warning',
    'critical'
);

-- Update notifications table schema
DROP TABLE IF EXISTS notifications CASCADE;

CREATE TABLE notifications (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Content
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    
    -- Classification
    notification_type notification_type NOT NULL,
    severity notification_severity NOT NULL DEFAULT 'info',
    
    -- Action
    action_url TEXT, -- Deep link for user action (e.g., "/dashboard/upgrade")
    
    -- Metadata (extensible JSON for context)
    metadata JSONB,
    
    -- Status
    is_read BOOLEAN NOT NULL DEFAULT FALSE,
    read_at TIMESTAMPTZ,
    
    -- Lifecycle
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ, -- Auto-delete after this date
    
    -- Constraints
    CONSTRAINT valid_read_at CHECK (read_at IS NULL OR is_read = TRUE)
);

-- Indexes for fast queries
CREATE INDEX idx_notifications_user_id ON notifications(user_id);
CREATE INDEX idx_notifications_user_id_is_read ON notifications(user_id, is_read);
CREATE INDEX idx_notifications_user_id_created_at ON notifications(user_id, created_at DESC);
CREATE INDEX idx_notifications_type ON notifications(notification_type);
CREATE INDEX idx_notifications_severity ON notifications(severity);
CREATE INDEX idx_notifications_expires_at ON notifications(expires_at) 
    WHERE expires_at IS NOT NULL;

-- Composite index for common queries
CREATE INDEX idx_notifications_user_unread ON notifications(user_id, created_at DESC) 
    WHERE is_read = FALSE;

-- ============================================================================
-- TRIGGERS
-- ============================================================================

-- Auto-update updated_at timestamp
CREATE OR REPLACE FUNCTION update_notifications_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_notifications_updated_at
    BEFORE UPDATE ON notifications
    FOR EACH ROW
    EXECUTE FUNCTION update_notifications_updated_at();

-- Auto-set read_at when marking as read
CREATE OR REPLACE FUNCTION set_notification_read_at()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.is_read = TRUE AND OLD.is_read = FALSE THEN
        NEW.read_at = NOW();
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_set_notification_read_at
    BEFORE UPDATE ON notifications
    FOR EACH ROW
    EXECUTE FUNCTION set_notification_read_at();

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Function to clean up expired notifications
CREATE OR REPLACE FUNCTION cleanup_expired_notifications()
RETURNS TABLE(deleted_count BIGINT) AS $$
BEGIN
    DELETE FROM notifications 
    WHERE expires_at IS NOT NULL 
        AND expires_at < NOW();
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- Function to clean up old read notifications
CREATE OR REPLACE FUNCTION cleanup_old_read_notifications(retention_days INTEGER)
RETURNS TABLE(deleted_count BIGINT) AS $$
BEGIN
    DELETE FROM notifications 
    WHERE is_read = TRUE 
        AND read_at < NOW() - (retention_days || ' days')::INTERVAL;
    
    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN NEXT;
END;
$$ LANGUAGE plpgsql;

-- Function to get unread notification count for a user
CREATE OR REPLACE FUNCTION get_unread_notification_count(p_user_id UUID)
RETURNS BIGINT AS $$
DECLARE
    unread_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO unread_count
    FROM notifications
    WHERE user_id = p_user_id
        AND is_read = FALSE
        AND (expires_at IS NULL OR expires_at > NOW());
    
    RETURN unread_count;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- VIEWS (Optional - for analytics)
-- ============================================================================

-- Notification summary by type and severity
CREATE OR REPLACE VIEW notification_summary AS
SELECT 
    user_id,
    notification_type,
    severity,
    COUNT(*) as total_count,
    COUNT(*) FILTER (WHERE is_read = FALSE) as unread_count,
    MAX(created_at) as latest_notification
FROM notifications
WHERE expires_at IS NULL OR expires_at > NOW()
GROUP BY user_id, notification_type, severity;

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON TABLE notifications IS 'User notifications with classification and expiry';
COMMENT ON COLUMN notifications.notification_type IS 'Category of notification for filtering';
COMMENT ON COLUMN notifications.severity IS 'Priority level (info, warning, critical)';
COMMENT ON COLUMN notifications.action_url IS 'Deep link for user action (e.g., upgrade page)';
COMMENT ON COLUMN notifications.metadata IS 'Extensible JSON context (e.g., quota percentages)';
COMMENT ON COLUMN notifications.expires_at IS 'Auto-delete notification after this date';

-- ============================================================================
-- SAMPLE DATA (for testing - remove in production)
-- ============================================================================

-- Insert sample notification (requires existing user)
-- INSERT INTO notifications (
--     user_id,
--     title,
--     message,
--     notification_type,
--     severity,
--     action_url,
--     metadata,
--     expires_at
-- )
-- SELECT 
--     id as user_id,
--     'Welcome to Vaultless Data!' as title,
--     'Get started by creating your first API key.' as message,
--     'system_update' as notification_type,
--     'info' as severity,
--     '/dashboard/keys' as action_url,
--     '{"welcome": true}'::jsonb as metadata,
--     NOW() + INTERVAL '30 days' as expires_at
-- FROM users
-- LIMIT 1;