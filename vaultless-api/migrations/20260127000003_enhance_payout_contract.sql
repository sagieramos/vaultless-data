-- Migration: Enhance psp_payouts table for PSP-agnostic processing
-- Description: Add fields to support currency conversion, FX rates, and normalized PSP responses

BEGIN;

-- 1. Add currency-related fields to psp_payouts table
ALTER TABLE psp_payouts
ADD COLUMN IF NOT EXISTS source_currency VARCHAR(3) NOT NULL DEFAULT 'USD',  -- Settlement currency
ADD COLUMN IF NOT EXISTS destination_currency VARCHAR(3) NOT NULL DEFAULT 'USD',  -- Developer's preferred currency
ADD COLUMN IF NOT EXISTS requested_amount BIGINT NOT NULL DEFAULT 0,  -- Amount requested in source currency
ADD COLUMN IF NOT EXISTS converted_amount BIGINT NOT NULL DEFAULT 0,  -- Amount after currency conversion
ADD COLUMN IF NOT EXISTS fx_rate DECIMAL(10,6),  -- Exchange rate used for conversion
ADD COLUMN IF NOT EXISTS psp_fee_deducted BIGINT NOT NULL DEFAULT 0,  -- Fee charged by PSP
ADD COLUMN IF NOT EXISTS net_paid_amount BIGINT NOT NULL DEFAULT 0,  -- Net amount paid to developer after fees
ADD COLUMN IF NOT EXISTS settlement_date TIMESTAMPTZ,  -- Expected settlement date from PSP
ADD COLUMN IF NOT EXISTS psp_normalized_response JSONB;  -- Normalized response from PSP

-- 2. Update the amount_cents column to be more descriptive (keeping for compatibility)
-- We'll use requested_amount for the main amount field going forward
ALTER TABLE psp_payouts ALTER COLUMN amount_cents SET DEFAULT 0;

-- 3. Add indexes for the new currency fields
CREATE INDEX IF NOT EXISTS idx_psp_payouts_source_currency ON psp_payouts(source_currency);
CREATE INDEX IF NOT EXISTS idx_psp_payouts_dest_currency ON psp_payouts(destination_currency);
CREATE INDEX IF NOT EXISTS idx_psp_payouts_settlement_date ON psp_payouts(settlement_date);

COMMIT;