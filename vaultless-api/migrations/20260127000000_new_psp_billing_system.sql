-- Migration: Create tables for new PSP-integrated billing system
-- Description: Refactor billing and payout system so platform never holds withdrawable money
--              Treat client credits as non-cash usage units
--              Attribute revenue to developers based on usage events
--              Delegate all money custody and payouts to a PSP
--              Platform takes a percentage per transaction

BEGIN;

-- 1. Create psp_accounts table
CREATE TABLE psp_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    developer_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- PSP-specific identifiers
    psp_account_id VARCHAR(255) NOT NULL,  -- Account ID in the PSP system
    psp_customer_id VARCHAR(255),          -- Customer ID in the PSP system

    -- Account details
    account_type VARCHAR(50) NOT NULL,     -- e.g., 'bank_account', 'paypal', 'stripe'
    account_details JSONB,                 -- Bank details, PayPal email, etc.

    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    is_verified BOOLEAN NOT NULL DEFAULT FALSE,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    UNIQUE(developer_id, is_active),
    UNIQUE(psp_account_id)
);

-- Create indexes for psp_accounts
CREATE INDEX idx_psp_accounts_developer ON psp_accounts(developer_id);
CREATE INDEX idx_psp_accounts_active ON psp_accounts(is_active) WHERE is_active = TRUE;

-- 2. Create developer_revenue_shares table
CREATE TABLE developer_revenue_shares (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Developer and application info
    developer_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,

    -- Usage period
    billing_period_id UUID NOT NULL REFERENCES billing_periods(id) ON DELETE CASCADE,

    -- Usage metrics that generated revenue
    messages_processed BIGINT NOT NULL DEFAULT 0,
    bytes_transferred BIGINT NOT NULL DEFAULT 0,
    proofs_verified BIGINT NOT NULL DEFAULT 0,

    -- Revenue calculation (ACCOUNTING METADATA ONLY - not real money held by platform)
    gross_revenue_cents BIGINT NOT NULL,        -- Total revenue from usage (accounting metadata)
    platform_fee_percent DECIMAL(5,2) NOT NULL, -- Platform percentage (e.g., 10.00 for 10%)
    platform_fee_cents BIGINT NOT NULL,         -- Calculated platform fee (accounting metadata)
    net_revenue_cents BIGINT NOT NULL,          -- What goes to developer (accounting metadata)

    -- PSP transaction info
    psp_transaction_id VARCHAR(255),            -- Transaction ID from PSP
    psp_payout_id VARCHAR(255),                 -- Payout ID from PSP

    -- Status and tracking
    status VARCHAR(50) NOT NULL DEFAULT 'pending_settlement'
           CHECK (status IN ('pending_settlement', 'settled', 'paid_to_developer', 'failed')),

    -- Timestamps
    calculated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    settled_at TIMESTAMPTZ,
    paid_at TIMESTAMPTZ
);

-- Create indexes for developer_revenue_shares
CREATE INDEX idx_dev_rev_share_developer ON developer_revenue_shares(developer_id);
CREATE INDEX idx_dev_rev_share_period ON developer_revenue_shares(billing_period_id);
CREATE INDEX idx_dev_rev_share_status ON developer_revenue_shares(status);
CREATE INDEX idx_dev_rev_share_application ON developer_revenue_shares(application_id);
CREATE INDEX idx_dev_rev_share_dev_period ON developer_revenue_shares(developer_id, billing_period_id);

-- 3. Create client_usage_credits table (NON-CASH UNITS ONLY)
CREATE TABLE client_usage_credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Client and application
    client_id UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,

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
    UNIQUE(client_id, application_id)
);

-- Create indexes for client_usage_credits
CREATE INDEX idx_client_credits_client ON client_usage_credits(client_id);
CREATE INDEX idx_client_credits_application ON client_usage_credits(application_id);
CREATE INDEX idx_client_credits_balance ON client_usage_credits(credit_balance);
CREATE INDEX idx_client_credits_expire ON client_usage_credits(expires_at) WHERE expires_at IS NOT NULL;

-- 4. Create credit_transactions table (for audit trail of non-cash credit movements)
CREATE TABLE credit_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Related entities
    client_id UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,

    -- Transaction details
    transaction_type VARCHAR(50) NOT NULL
                     CHECK (transaction_type IN ('credit_purchase', 'credit_allocation', 'usage_deduction', 'refund', 'expiration')),
    amount BIGINT NOT NULL,                    -- Amount of credits affected (can be negative, non-cash units)

    -- Usage context (when credits are consumed)
    usage_context JSONB,                       -- Details about what the credits were used for

    -- Cash equivalent (for accounting purposes only - not real money held by platform)
    cash_equivalent_cents BIGINT NOT NULL DEFAULT 0,  -- Cash value of this transaction (accounting metadata)

    -- References
    related_transaction_id UUID,               -- For refunds/reversals
    billing_period_id UUID REFERENCES billing_periods(id),  -- Associated billing period

    -- Status
    status VARCHAR(50) NOT NULL DEFAULT 'completed'
           CHECK (status IN ('pending', 'completed', 'failed', 'reversed')),

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for credit_transactions
CREATE INDEX idx_credit_trans_client ON credit_transactions(client_id);
CREATE INDEX idx_credit_trans_application ON credit_transactions(application_id);
CREATE INDEX idx_credit_trans_type ON credit_transactions(transaction_type);
CREATE INDEX idx_credit_trans_period ON credit_transactions(billing_period_id);

-- 5. Create psp_payouts table (PSP handles ALL money movement)
CREATE TABLE psp_payouts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Developer and PSP account
    developer_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    psp_account_id UUID NOT NULL REFERENCES psp_accounts(id) ON DELETE CASCADE,

    -- Payout details (AMOUNT REQUESTED FROM PSP - platform never holds these funds)
    amount_cents BIGINT NOT NULL,              -- Amount to payout (amount sent to PSP for payout)
    currency VARCHAR(3) NOT NULL DEFAULT 'USD', -- Currency code

    -- PSP integration (PSP is SINGLE SOURCE OF TRUTH for money movement)
    psp_payout_request_id VARCHAR(255),        -- Request ID in PSP system
    psp_payout_status VARCHAR(50),             -- Status from PSP (source of truth)
    psp_response_data JSONB,                   -- Raw response from PSP (source of truth)

    -- Tracking revenue shares included in this payout
    revenue_share_ids UUID[],                  -- Array of revenue share IDs included

    -- Status and timing
    status VARCHAR(50) NOT NULL DEFAULT 'pending'
           CHECK (status IN ('pending', 'processing', 'sent', 'delivered', 'failed', 'cancelled')),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,

    -- Failure tracking
    failure_reason TEXT
);

-- Create indexes for psp_payouts
CREATE INDEX idx_psp_payouts_developer ON psp_payouts(developer_id);
CREATE INDEX idx_psp_payouts_status ON psp_payouts(status);
CREATE INDEX idx_psp_payouts_requested_at ON psp_payouts(requested_at);
CREATE INDEX idx_psp_payouts_dev_status ON psp_payouts(developer_id, status);

-- 6. Modify billing_periods to support PSP integration
ALTER TABLE billing_periods
ADD COLUMN psp_processing_status VARCHAR(50) DEFAULT 'not_started'
       CHECK (psp_processing_status IN ('not_started', 'processing', 'completed', 'failed')),
ADD COLUMN psp_processing_completed_at TIMESTAMPTZ,
ADD COLUMN platform_revenue_cents BIGINT NOT NULL DEFAULT 0;  -- Accounting metadata only

-- 7. Modify client_invoices to work with the new system
ALTER TABLE client_invoices
ADD COLUMN is_billable_to_client BOOLEAN NOT NULL DEFAULT TRUE,  -- Whether this is billable to the client
ADD COLUMN psp_invoice_id VARCHAR(255),                          -- Invoice ID in PSP system
ADD COLUMN converted_to_credits BOOLEAN NOT NULL DEFAULT FALSE;  -- Whether invoice was converted to credits

-- Add comments to clarify that these are accounting metadata, not real money held by platform
COMMENT ON COLUMN developer_revenue_shares.gross_revenue_cents IS 'Gross revenue in cents (accounting metadata only, not funds held by platform)';
COMMENT ON COLUMN developer_revenue_shares.platform_fee_cents IS 'Platform fee in cents (accounting metadata only, not funds held by platform)';
COMMENT ON COLUMN developer_revenue_shares.net_revenue_cents IS 'Net revenue for developer in cents (accounting metadata only, not funds held by platform)';
COMMENT ON COLUMN client_usage_credits.estimated_cash_value_cents IS 'Estimated cash value of credits (accounting metadata only, not funds held by platform)';
COMMENT ON COLUMN credit_transactions.cash_equivalent_cents IS 'Cash equivalent value (accounting metadata only, not funds held by platform)';
COMMENT ON COLUMN psp_payouts.amount_cents IS 'Amount requested from PSP for payout (platform never holds these funds)';

COMMIT;