-- Migration: Create pricing snapshots table

-- This table stores pricing configurations at a point in time
-- so that billing calculations are consistent and auditable

CREATE TABLE IF NOT EXISTS pricing_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Links to the original pricing plan
    pricing_plan_id UUID REFERENCES pricing_plans(id),
    
    -- Plan metadata
    plan_name VARCHAR(255) NOT NULL,
    pricing_mode pricing_mode_enum NOT NULL DEFAULT 'postpaid',
    
    -- Pricing tiers (in cents)
    price_per_message_cents INTEGER,  -- Price per message (in cents)
    price_per_gb_cents INTEGER,       -- Price per GB transferred (in cents)
    price_per_proof_cents INTEGER,    -- Price per proof verified (in cents)
    prepaid_amount_cents BIGINT,      -- Prepaid amount (for prepaid plans)
    
    -- Currency and conversion
    currency_code CHAR(3) NOT NULL DEFAULT 'USD',  -- ISO 4217 currency code
    
    -- Validity period
    valid_from TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    valid_until TIMESTAMP WITH TIME ZONE,
    
    -- Creation metadata
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_pricing_snapshots_plan ON pricing_snapshots(pricing_plan_id);
CREATE INDEX IF NOT EXISTS idx_pricing_snapshots_validity ON pricing_snapshots(valid_from, valid_until);
CREATE INDEX IF NOT EXISTS idx_pricing_snapshots_currency ON pricing_snapshots(currency_code);

-- Trigger to update the updated_at timestamp
CREATE TRIGGER update_pricing_snapshots_updated_at
    BEFORE UPDATE ON pricing_snapshots
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();