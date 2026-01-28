-- Migration: Remove estimated_cash_value_cents from client_usage_credits
-- Description: Remove currency-specific field to make the system currency-agnostic
--              All accounting can be derived from credit purchase events, usage events,
--              and billing periods. Real accounting happens at the PSP level.

BEGIN;

-- 1. Remove the estimated_cash_value_cents column from client_usage_credits
ALTER TABLE client_usage_credits 
DROP COLUMN estimated_cash_value_cents;

-- 2. Remove the estimated_cash_value_cents column from clients table if it exists
ALTER TABLE clients 
DROP COLUMN IF EXISTS estimated_cash_value_cents;

-- 3. Remove the cash_equivalent_cents column from credit_transactions as well
--    since we're moving to a currency-neutral model
ALTER TABLE credit_transactions 
DROP COLUMN cash_equivalent_cents;

-- 4. Add a currency column to credit_transactions to track the currency of each transaction
ALTER TABLE credit_transactions
ADD COLUMN currency_code VARCHAR(3) NOT NULL DEFAULT 'USD';  -- ISO 4217 currency code

-- 5. Add a credit_unit_value column to track the value of credits at time of purchase
--    This preserves the economic relationship without assuming a specific currency
ALTER TABLE credit_transactions
ADD COLUMN credit_unit_value BIGINT NOT NULL DEFAULT 0;  -- Value of 1 credit unit in smallest currency denomination (e.g., cents for USD)

COMMIT;