-- Comprehensive billing system with invoices, payments, subscriptions, and credits

-- ============================================================================
-- ENUMS
-- ============================================================================

CREATE TYPE invoice_status AS ENUM (
    'draft',
    'open',
    'paid',
    'void',
    'uncollectible'
);

CREATE TYPE line_item_type AS ENUM (
    'subscription',
    'message_overage',
    'storage_overage',
    'proof_verification',
    'setup',
    'discount',
    'tax',
    'credit'
);

CREATE TYPE payment_method AS ENUM (
    'card',
    'bank_transfer',
    'crypto',
    'paypal',
    'other'
);

CREATE TYPE payment_status AS ENUM (
    'pending',
    'processing',
    'succeeded',
    'failed',
    'canceled',
    'refunded'
);

CREATE TYPE subscription_status AS ENUM (
    'trialing',
    'active',
    'past_due',
    'canceled',
    'unpaid',
    'incomplete'
);

CREATE TYPE billing_cycle AS ENUM (
    'monthly',
    'yearly'
);

CREATE TYPE credit_transaction_type AS ENUM (
    'purchase',
    'bonus',
    'refund',
    'applied',
    'expired',
    'adjustment'
);

-- ============================================================================
-- INVOICES TABLE
-- ============================================================================

CREATE TABLE invoices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    
    -- Stripe integration
    stripe_invoice_id VARCHAR(255) UNIQUE,
    stripe_subscription_id VARCHAR(255),
    
    -- Amounts (in cents to avoid floating point issues)
    subtotal_cents BIGINT NOT NULL CHECK (subtotal_cents >= 0),
    tax_cents BIGINT NOT NULL DEFAULT 0 CHECK (tax_cents >= 0),
    discount_cents BIGINT NOT NULL DEFAULT 0 CHECK (discount_cents >= 0),
    total_cents BIGINT NOT NULL CHECK (total_cents >= 0),
    amount_paid_cents BIGINT NOT NULL DEFAULT 0 CHECK (amount_paid_cents >= 0),
    amount_due_cents BIGINT NOT NULL CHECK (amount_due_cents >= 0),
    
    -- Invoice details
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    invoice_number VARCHAR(50) NOT NULL UNIQUE,
    description TEXT,
    
    -- Status
    status invoice_status NOT NULL DEFAULT 'open',
    paid BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Dates
    billing_period_start TIMESTAMPTZ NOT NULL,
    billing_period_end TIMESTAMPTZ NOT NULL,
    due_date TIMESTAMPTZ NOT NULL,
    paid_at TIMESTAMPTZ,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata (extensible JSON)
    metadata JSONB,
    
    -- Constraints
    CONSTRAINT valid_billing_period CHECK (billing_period_end > billing_period_start),
    CONSTRAINT valid_due_date CHECK (due_date >= billing_period_end),
    CONSTRAINT valid_paid_status CHECK (
        (paid = TRUE AND paid_at IS NOT NULL AND status = 'paid') OR
        (paid = FALSE AND (paid_at IS NULL OR status != 'paid'))
    ),
    CONSTRAINT valid_amounts CHECK (
        total_cents = subtotal_cents + tax_cents - discount_cents AND
        amount_paid_cents <= total_cents AND
        amount_due_cents = total_cents - amount_paid_cents
    )
);

CREATE INDEX idx_invoices_user_id ON invoices(user_id);
CREATE INDEX idx_invoices_api_key_id ON invoices(api_key_id) WHERE api_key_id IS NOT NULL;
CREATE INDEX idx_invoices_status ON invoices(status);