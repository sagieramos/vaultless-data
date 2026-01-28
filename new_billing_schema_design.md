# New Billing and Payout System Schema Design

## Overview
The new system will refactor the billing and payout system so that:
1. The platform never holds withdrawable money
2. Client credits are treated as non-cash usage units
3. Revenue is attributed to developers strictly based on usage events
4. All money custody and payouts are delegated to a PSP
5. Platform takes a percentage per transaction

## New Tables to Create

### 1. psp_accounts
Stores PSP account information for developers

```sql
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
```

### 2. developer_revenue_shares
Tracks revenue shares for developers based on usage events

```sql
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
    
    -- Revenue calculation
    gross_revenue_cents BIGINT NOT NULL,        -- Total revenue from usage
    platform_fee_percent DECIMAL(5,2) NOT NULL, -- Platform percentage (e.g., 10.00 for 10%)
    platform_fee_cents BIGINT NOT NULL,         -- Calculated platform fee
    net_revenue_cents BIGINT NOT NULL,          -- What goes to developer
    
    -- PSP transaction info
    psp_transaction_id VARCHAR(255),            -- Transaction ID from PSP
    psp_payout_id VARCHAR(255),                 -- Payout ID from PSP
    
    -- Status and tracking
    status VARCHAR(50) NOT NULL DEFAULT 'pending_settlement' 
           CHECK (status IN ('pending_settlement', 'settled', 'paid_to_developer', 'failed')),
    
    -- Timestamps
    calculated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    settled_at TIMESTAMPTZ,
    paid_at TIMESTAMPTZ,
    
    -- Indexes
    INDEX idx_dev_rev_share_dev_period (developer_id, billing_period_id),
    INDEX idx_dev_rev_share_status (status),
    INDEX idx_dev_rev_share_app (application_id)
);
```

### 3. client_usage_credits
Tracks client usage credits as non-cash units

```sql
CREATE TABLE client_usage_credits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Client and application
    client_id UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    
    -- Credit balance (non-cash units)
    credit_balance BIGINT NOT NULL DEFAULT 0,    -- Available credit units
    credit_consumed BIGINT NOT NULL DEFAULT 0,   -- Consumed credit units
    credit_provided BIGINT NOT NULL DEFAULT 0,   -- Total credits ever provided
    
    -- Credit value tracking (for accounting)
    estimated_cash_value_cents BIGINT NOT NULL DEFAULT 0,  -- Estimated cash value of credits
    
    -- Expiration (optional)
    expires_at TIMESTAMPTZ,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Constraints
    UNIQUE(client_id, application_id),
    
    -- Indexes
    INDEX idx_client_credits_balance (credit_balance),
    INDEX idx_client_credits_expire (expires_at) WHERE expires_at IS NOT NULL
);
```

### 4. credit_transactions
Logs all credit transactions for audit trail

```sql
CREATE TABLE credit_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Related entities
    client_id UUID NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    application_id UUID NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
    
    -- Transaction details
    transaction_type VARCHAR(50) NOT NULL 
                     CHECK (transaction_type IN ('credit_purchase', 'credit_allocation', 'usage_deduction', 'refund', 'expiration')),
    amount BIGINT NOT NULL,                    -- Amount of credits affected (can be negative)
    
    -- Usage context (when credits are consumed)
    usage_context JSONB,                       -- Details about what the credits were used for
    
    -- Cash equivalent (for accounting)
    cash_equivalent_cents BIGINT NOT NULL DEFAULT 0,  -- Cash value of this transaction
    
    -- References
    related_transaction_id UUID,               -- For refunds/reversals
    billing_period_id UUID REFERENCES billing_periods(id),  -- Associated billing period
    
    -- Status
    status VARCHAR(50) NOT NULL DEFAULT 'completed'
           CHECK (status IN ('pending', 'completed', 'failed', 'reversed')),
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Indexes
    INDEX idx_credit_trans_client (client_id),
    INDEX idx_credit_trans_app (application_id),
    INDEX idx_credit_trans_type (transaction_type),
    INDEX idx_credit_trans_period (billing_period_id)
);
```

### 5. psp_payouts
Manages automated payouts to developers

```sql
CREATE TABLE psp_payouts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- Developer and PSP account
    developer_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    psp_account_id UUID NOT NULL REFERENCES psp_accounts(id) ON DELETE CASCADE,
    
    -- Payout details
    amount_cents BIGINT NOT NULL,              -- Amount to payout
    currency VARCHAR(3) NOT NULL DEFAULT 'USD', -- Currency code
    
    -- PSP integration
    psp_payout_request_id VARCHAR(255),        -- Request ID in PSP system
    psp_payout_status VARCHAR(50),             -- Status from PSP
    psp_response_data JSONB,                   -- Raw response from PSP
    
    -- Tracking revenue shares included in this payout
    revenue_share_ids UUID[],                  -- Array of revenue share IDs included
    
    -- Status and timing
    status VARCHAR(50) NOT NULL DEFAULT 'pending' 
           CHECK (status IN ('pending', 'processing', 'sent', 'delivered', 'failed', 'cancelled')),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    
    -- Failure tracking
    failure_reason TEXT,
    
    -- Indexes
    INDEX idx_psp_payouts_dev_status (developer_id, status),
    INDEX idx_psp_payouts_status (status),
    INDEX idx_psp_payouts_req_time (requested_at)
);
```

## Modified Tables

### 6. Modify billing_periods to support PSP integration
```sql
-- Add PSP-related fields to billing_periods
ALTER TABLE billing_periods 
ADD COLUMN psp_processing_status VARCHAR(50) DEFAULT 'not_started'
       CHECK (psp_processing_status IN ('not_started', 'processing', 'completed', 'failed')),
ADD COLUMN psp_processing_completed_at TIMESTAMPTZ,
ADD COLUMN platform_revenue_cents BIGINT NOT NULL DEFAULT 0;
```

### 7. Modify client_invoices to work with the new system
```sql
-- Add fields to track if invoice was processed through PSP
ALTER TABLE client_invoices
ADD COLUMN is_billable_to_client BOOLEAN NOT NULL DEFAULT TRUE,  -- Whether this is billable to the client
ADD COLUMN psp_invoice_id VARCHAR(255),                          -- Invoice ID in PSP system
ADD COLUMN converted_to_credits BOOLEAN NOT NULL DEFAULT FALSE;  -- Whether invoice was converted to credits
```

## Key Business Logic Changes

1. **Revenue Attribution**: Instead of accumulating funds in platform accounts, revenue from client usage is immediately attributed to developers based on usage events.

2. **Non-Cash Credits**: Client credits become usage units rather than cash equivalents. When clients purchase credits, they're buying usage capacity, not storing value.

3. **PSP Integration**: All monetary transactions flow through a Payment Service Provider (PSP) like Stripe Connect, PayPal, or similar.

4. **Automated Payouts**: Developer revenue is automatically calculated and paid out through the PSP without platform holding funds.

5. **Audit Trail**: Every credit transaction is logged for compliance and accounting purposes.

## Process Flow

1. Client purchases credits → Funds go directly to PSP
2. Client uses service → Credits are consumed based on usage
3. Usage generates revenue attribution to developer
4. At billing period close → Calculate developer revenue shares
5. Platform takes percentage → Pay remaining to developer via PSP
6. Developer receives payout directly from PSP to their account