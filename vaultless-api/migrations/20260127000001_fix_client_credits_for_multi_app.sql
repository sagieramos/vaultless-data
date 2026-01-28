-- Migration: Fix client_usage_credits to be application-agnostic
-- Description: Client credits should be usable across any application,
--              not tied to a specific application

BEGIN;

-- 1. Create a temporary table to preserve existing data
CREATE TEMPORARY TABLE temp_client_credits AS 
SELECT 
    client_id,
    SUM(credit_balance) as total_balance,
    SUM(credit_consumed) as total_consumed,
    SUM(credit_provided) as total_provided,
    SUM(estimated_cash_value_cents) as total_cash_value
FROM client_usage_credits
GROUP BY client_id;

-- 2. Drop the old client_usage_credits table
DROP TABLE client_usage_credits;

-- 3. Create the new client_usage_credits table without application_id
CREATE TABLE client_usage_credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Client only (credits are application-agnostic)
    client_id UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    
    -- Credit balance (non-cash units - these unlock usage, not real money)
    credit_balance BIGINT NOT NULL DEFAULT 0,    -- Available credit units (non-cash)
    credit_consumed BIGINT NOT NULL DEFAULT 0,   -- Consumed credit units (non-cash)
    credit_provided BIGINT NOT NULL DEFAULT 0,   -- Total credits ever provided (non-cash)
    
    -- Credit value tracking (for accounting purposes only - not real money)
    estimated_cash_value_cents BIGINT NOT NULL DEFAULT 0,  -- Estimated cash value of credits (accounting metadata)
    
    -- Expiration (optional)
    expires_at TIMESTAMPTZ,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Constraints
    UNIQUE(client_id)  -- Each client has one credit record
);

-- 4. Populate the new client_usage_credits table with aggregated data
INSERT INTO client_usage_credits (client_id, credit_balance, credit_consumed, credit_provided, estimated_cash_value_cents, created_at, updated_at)
SELECT 
    client_id,
    total_balance,
    total_consumed,
    total_provided,
    total_cash_value,
    NOW() as created_at,
    NOW() as updated_at
FROM temp_client_credits;

-- 5. Create indexes for the new client_usage_credits table
CREATE INDEX idx_client_credits_client ON client_usage_credits(client_id);
CREATE INDEX idx_client_credits_balance ON client_usage_credits(credit_balance);
CREATE INDEX idx_client_credits_expire ON client_usage_credits(expires_at) WHERE expires_at IS NOT NULL;

COMMIT;