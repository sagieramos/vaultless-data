-- ============================================================================
-- VAULTLESS GATEWAY-AGNOSTIC BILLING SYSTEM
-- Supports Stripe, Paystack, Razorpay, Flutterwave, etc.
-- ============================================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "timescaledb";

-- ============================================================================
-- ENUM TYPES
-- ============================================================================

CREATE TYPE payment_gateway AS ENUM (
    'stripe',
    'paypal',
    'paystack',
    'flutterwave',
    'razorpay',
    'square',
    'braintree',
    'manual'
);

CREATE TYPE invoice_status AS ENUM (
    'draft',
    'open',
    'uncollectible',
    'paid',
    'void'
);

CREATE TYPE payment_status AS ENUM (
    'pending',
    'succeeded',
    'failed',
    'refunded',
    'canceled'
);

CREATE TYPE payment_method AS ENUM (
    'card',
    'bank_transfer',
    'mobile_money',
    'paypal',
    'crypto',
    'manual'
);

CREATE TYPE subscription_status AS ENUM (
    'trialing',
    'active',
    'past_due',
    'canceled',
    'unpaid',
    'paused'
);

CREATE TYPE billing_cycle AS ENUM ('monthly', 'yearly');

CREATE TYPE line_item_type AS ENUM (
    'usage',
    'subscription',
    'discount',
    'tax'
);

CREATE TYPE credit_transaction_type AS ENUM (
    'credit',
    'debit'
);

-- ============================================================================
-- UNIVERSAL GATEWAY REFERENCE TABLE
-- ============================================================================

CREATE TABLE external_payment_references (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    entity_type VARCHAR(50) NOT NULL,  -- 'invoice', 'subscription', 'payment', 'customer'
    entity_id UUID NOT NULL,
    gateway payment_gateway NOT NULL,
    external_id VARCHAR(255) NOT NULL,
    external_metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (entity_type, entity_id, gateway)
);

CREATE INDEX idx_external_refs_entity ON external_payment_references(entity_type, entity_id);
CREATE INDEX idx_external_refs_gateway ON external_payment_references(gateway, external_id);
CREATE INDEX idx_external_refs_lookup ON external_payment_references(entity_id, gateway);

-- ============================================================================
-- INVOICES TABLE
-- ============================================================================

CREATE TABLE invoices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    payment_gateway payment_gateway,
    subtotal_cents BIGINT NOT NULL CHECK (subtotal_cents >= 0),
    tax_cents BIGINT NOT NULL DEFAULT 0 CHECK (tax_cents >= 0),
    discount_cents BIGINT NOT NULL DEFAULT 0 CHECK (discount_cents >= 0),
    total_cents BIGINT NOT NULL CHECK (total_cents >= 0),
    amount_paid_cents BIGINT NOT NULL DEFAULT 0 CHECK (amount_paid_cents >= 0),
    amount_due_cents BIGINT NOT NULL DEFAULT 0 CHECK (amount_due_cents >= 0),
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    invoice_number VARCHAR(50) NOT NULL UNIQUE,
    description TEXT,
    status invoice_status NOT NULL DEFAULT 'open',
    paid BOOLEAN NOT NULL DEFAULT FALSE,
    billing_period_start TIMESTAMPTZ NOT NULL,
    billing_period_end TIMESTAMPTZ NOT NULL,
    due_date TIMESTAMPTZ NOT NULL,
    paid_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB,
    CONSTRAINT valid_billing_period CHECK (billing_period_end > billing_period_start),
    CONSTRAINT valid_due_date CHECK (due_date >= billing_period_end),
    CONSTRAINT valid_paid_status CHECK (
        (paid = TRUE AND paid_at IS NOT NULL AND status = 'paid')
        OR (paid = FALSE AND (paid_at IS NULL OR status != 'paid'))
    ),
    CONSTRAINT valid_amounts CHECK (
        total_cents = subtotal_cents + tax_cents - discount_cents
        AND amount_paid_cents <= total_cents
        AND amount_due_cents = total_cents - amount_paid_cents
    )
);

CREATE INDEX idx_invoices_user_id ON invoices(user_id);
CREATE INDEX idx_invoices_api_key_id ON invoices(api_key_id) WHERE api_key_id IS NOT NULL;
CREATE INDEX idx_invoices_status ON invoices(status);
CREATE INDEX idx_invoices_paid ON invoices(paid, due_date) WHERE paid = FALSE;
CREATE INDEX idx_invoices_gateway ON invoices(payment_gateway) WHERE payment_gateway IS NOT NULL;

-- ============================================================================
-- INVOICE LINE ITEMS
-- ============================================================================

CREATE TABLE invoice_line_items (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    description TEXT NOT NULL,
    item_type line_item_type NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    unit_price_cents BIGINT NOT NULL,
    amount_cents BIGINT NOT NULL CHECK (amount_cents = quantity * unit_price_cents),
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_line_items_invoice_id ON invoice_line_items(invoice_id);
CREATE INDEX idx_line_items_api_key_id ON invoice_line_items(api_key_id) WHERE api_key_id IS NOT NULL;

-- ============================================================================
-- PAYMENTS TABLE
-- ============================================================================

CREATE TABLE payments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    payment_gateway payment_gateway NOT NULL,
    amount_cents BIGINT NOT NULL CHECK (amount_cents > 0),
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    payment_method payment_method NOT NULL,
    status payment_status NOT NULL DEFAULT 'pending',
    card_last4 VARCHAR(4),
    card_brand VARCHAR(50),
    failure_code VARCHAR(100),
    failure_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    processed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB
);

CREATE INDEX idx_payments_invoice_id ON payments(invoice_id);
CREATE INDEX idx_payments_user_id ON payments(user_id);
CREATE INDEX idx_payments_status ON payments(status);
CREATE INDEX idx_payments_gateway ON payments(payment_gateway);

-- ============================================================================
-- SUBSCRIPTIONS TABLE
-- ============================================================================

CREATE TABLE subscriptions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    payment_gateway payment_gateway NOT NULL,
    tier subscription_tier NOT NULL,
    status subscription_status NOT NULL DEFAULT 'active',
    billing_cycle billing_cycle NOT NULL DEFAULT 'monthly',
    amount_cents BIGINT NOT NULL CHECK (amount_cents >= 0),
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    trial_end TIMESTAMPTZ,
    trial_days INTEGER CHECK (trial_days IS NULL OR trial_days > 0),
    current_period_start TIMESTAMPTZ NOT NULL,
    current_period_end TIMESTAMPTZ NOT NULL,
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    canceled_at TIMESTAMPTZ,
    cancellation_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB,
    CONSTRAINT valid_period CHECK (current_period_end > current_period_start)
);

CREATE INDEX idx_subscriptions_user_id ON subscriptions(user_id);
CREATE INDEX idx_subscriptions_status ON subscriptions(status);
CREATE INDEX idx_subscriptions_gateway ON subscriptions(payment_gateway);
CREATE INDEX idx_subscriptions_active ON subscriptions(user_id, status)
    WHERE status IN ('trialing', 'active');

-- ============================================================================
-- CREDIT BALANCES
-- ============================================================================

CREATE TABLE credit_balances (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    balance_cents BIGINT NOT NULL DEFAULT 0 CHECK (balance_cents >= 0),
    reserved_cents BIGINT NOT NULL DEFAULT 0 CHECK (reserved_cents >= 0),
    available_cents BIGINT NOT NULL DEFAULT 0 CHECK (
        available_cents = balance_cents - reserved_cents AND available_cents >= 0
    ),
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_credit_balances_user_id ON credit_balances(user_id);

-- ============================================================================
-- CREDIT TRANSACTIONS
-- ============================================================================

CREATE TABLE credit_transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credit_balance_id UUID NOT NULL REFERENCES credit_balances(id) ON DELETE CASCADE,
    amount_cents BIGINT NOT NULL,
    transaction_type credit_transaction_type NOT NULL,
    description TEXT NOT NULL,
    invoice_id UUID REFERENCES invoices(id) ON DELETE SET NULL,
    payment_id UUID REFERENCES payments(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB
);

CREATE INDEX idx_credit_transactions_user_id ON credit_transactions(user_id);
CREATE INDEX idx_credit_transactions_balance_id ON credit_transactions(credit_balance_id);

-- ============================================================================
-- PAYMENT METHODS TABLE
-- ============================================================================

CREATE TABLE payment_methods (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    payment_gateway payment_gateway NOT NULL,
    type payment_method NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    card_last4 VARCHAR(4),
    card_brand VARCHAR(50),
    card_exp_month INTEGER CHECK (card_exp_month >= 1 AND card_exp_month <= 12),
    card_exp_year INTEGER CHECK (card_exp_year >= 2024),
    bank_last4 VARCHAR(4),
    bank_name VARCHAR(255),
    mobile_number_last4 VARCHAR(4),
    mobile_provider VARCHAR(50),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB
);

CREATE INDEX idx_payment_methods_user_id ON payment_methods(user_id);
CREATE INDEX idx_payment_methods_gateway ON payment_methods(payment_gateway);
CREATE INDEX idx_payment_methods_default ON payment_methods(user_id, is_default)
    WHERE is_default = TRUE;

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

CREATE OR REPLACE FUNCTION get_external_id(
    p_entity_type VARCHAR,
    p_entity_id UUID,
    p_gateway payment_gateway
)
RETURNS VARCHAR AS $$
DECLARE
    v_external_id VARCHAR;
BEGIN
    SELECT external_id INTO v_external_id
    FROM external_payment_references
    WHERE entity_type = p_entity_type
      AND entity_id = p_entity_id
      AND gateway = p_gateway;
    RETURN v_external_id;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION set_external_id(
    p_entity_type VARCHAR,
    p_entity_id UUID,
    p_gateway payment_gateway,
    p_external_id VARCHAR,
    p_metadata JSONB DEFAULT NULL
)
RETURNS UUID AS $$
DECLARE
    v_ref_id UUID;
BEGIN
    INSERT INTO external_payment_references (
        entity_type, entity_id, gateway, external_id, external_metadata
    )
    VALUES (p_entity_type, p_entity_id, p_gateway, p_external_id, p_metadata)
    ON CONFLICT (entity_type, entity_id, gateway)
    DO UPDATE SET
        external_id = p_external_id,
        external_metadata = p_metadata,
        updated_at = NOW()
    RETURNING id INTO v_ref_id;

    RETURN v_ref_id;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON TABLE external_payment_references IS 'Universal mapping between internal entities and payment gateway IDs';
COMMENT ON TABLE invoices IS 'Gateway-agnostic invoices';
COMMENT ON TABLE subscriptions IS 'Gateway-agnostic subscriptions';
COMMENT ON TABLE payments IS 'Gateway-agnostic payment records';
COMMENT ON TABLE credit_balances IS 'Tracks user credits and usage balance';
COMMENT ON TABLE payment_methods IS 'Gateway-aware payment methods';
