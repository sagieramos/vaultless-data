-- ============================================================================
-- Migration: Add bandwidth quota to developer subscriptions
-- Description: Add monthly bandwidth quota to subscription plans alongside 
--              existing message quotas for comprehensive resource management
-- ============================================================================

BEGIN;

-- Add monthly_bandwidth_quota column to developer_subscriptions table
ALTER TABLE public.developer_subscriptions
ADD COLUMN IF NOT EXISTS monthly_bandwidth_quota bigint NOT NULL DEFAULT 1073741824; -- 1GB in bytes

-- Update the comment for the table to reflect the new column
COMMENT ON COLUMN public.developer_subscriptions.monthly_bandwidth_quota IS 'Monthly bandwidth quota in bytes. Used to limit data transfer for the subscription tier.';

-- Update the table comment to include information about bandwidth
COMMENT ON TABLE public.developer_subscriptions IS 'Developer subscription plans with message and bandwidth quotas';

COMMIT;