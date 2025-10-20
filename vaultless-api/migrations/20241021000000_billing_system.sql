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

-- NOTE: You will need to ensure 'subscription_tier' TYPE is defined 
-- in an earlier migration, as it is used here but not defined.
-- Assuming 'subscription_tier' TYPE exists for now.
-- IF it doesn't exist, this migration will fail on 'subscriptions' table creation.

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
CREATE INDEX idx_invoices_paid ON invoices(paid, due_date) WHERE paid = FALSE;
CREATE INDEX idx_invoices_stripe_invoice_id ON invoices(stripe_invoice_id) WHERE stripe_invoice_id IS NOT NULL;
CREATE INDEX idx_invoices_due_date ON invoices(due_date) WHERE paid = FALSE;

-- ============================================================================
-- INVOICE LINE ITEMS TABLE
-- ============================================================================

CREATE TABLE invoice_line_items (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    
    -- Item details
    description TEXT NOT NULL,
    item_type line_item_type NOT NULL,
    quantity BIGINT NOT NULL CHECK (quantity > 0),
    unit_price_cents BIGINT NOT NULL,
    amount_cents BIGINT NOT NULL CHECK (amount_cents = quantity * unit_price_cents),
    
    -- Metadata
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_line_items_invoice_id ON invoice_line_items(invoice_id);
CREATE INDEX idx_line_items_api_key_id ON invoice_line_items(api_key_id) WHERE api_key_id IS NOT NULL;
CREATE INDEX idx_line_items_type ON invoice_line_items(item_type);

-- ============================================================================
-- PAYMENTS TABLE
-- ============================================================================

CREATE TABLE payments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Stripe integration
    stripe_payment_intent_id VARCHAR(255) UNIQUE,
    stripe_charge_id VARCHAR(255),
    
    -- Payment details
    amount_cents BIGINT NOT NULL CHECK (amount_cents > 0),
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    payment_method payment_method NOT NULL,
    status payment_status NOT NULL DEFAULT 'pending',
    
    -- Card/Bank details (PCI-compliant - only last 4 digits)
    card_last4 VARCHAR(4),
    card_brand VARCHAR(50), -- visa, mastercard, amex, etc.
    
    -- Failure tracking
    failure_code VARCHAR(100),
    failure_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    
    -- Dates
    processed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata
    metadata JSONB
);

CREATE INDEX idx_payments_invoice_id ON payments(invoice_id);
CREATE INDEX idx_payments_user_id ON payments(user_id);
CREATE INDEX idx_payments_status ON payments(status);
CREATE INDEX idx_payments_stripe_payment_intent ON payments(stripe_payment_intent_id) WHERE stripe_payment_intent_id IS NOT NULL;
CREATE INDEX idx_payments_created_at ON payments(created_at DESC);

-- ============================================================================
-- SUBSCRIPTIONS TABLE
-- ============================================================================

CREATE TABLE subscriptions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Stripe integration
    stripe_subscription_id VARCHAR(255) NOT NULL UNIQUE,
    stripe_customer_id VARCHAR(255) NOT NULL,
    stripe_price_id VARCHAR(255) NOT NULL,
    
    -- Subscription details
    tier subscription_tier NOT NULL,
    status subscription_status NOT NULL DEFAULT 'active',
    
    -- Billing
    billing_cycle billing_cycle NOT NULL DEFAULT 'monthly',
    amount_cents BIGINT NOT NULL CHECK (amount_cents >= 0),
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    
    -- Trial
    trial_end TIMESTAMPTZ,
    trial_days INTEGER CHECK (trial_days IS NULL OR trial_days > 0),
    
    -- Current period
    current_period_start TIMESTAMPTZ NOT NULL,
    current_period_end TIMESTAMPTZ NOT NULL,
    
    -- Cancellation
    cancel_at_period_end BOOLEAN NOT NULL DEFAULT FALSE,
    canceled_at TIMESTAMPTZ,
    cancellation_reason TEXT,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata
    metadata JSONB,
    
    -- Constraints
    CONSTRAINT valid_period CHECK (current_period_end > current_period_start),
    CONSTRAINT valid_cancellation CHECK (
        (canceled_at IS NULL) OR 
        (canceled_at IS NOT NULL AND cancel_at_period_end = TRUE)
    )
);

CREATE INDEX idx_subscriptions_user_id ON subscriptions(user_id);
CREATE INDEX idx_subscriptions_status ON subscriptions(status);
CREATE INDEX idx_subscriptions_stripe_subscription_id ON subscriptions(stripe_subscription_id);
CREATE INDEX idx_subscriptions_stripe_customer_id ON subscriptions(stripe_customer_id);
CREATE INDEX idx_subscriptions_current_period_end ON subscriptions(current_period_end);
CREATE INDEX idx_subscriptions_active ON subscriptions(user_id, status) 
    WHERE status IN ('trialing', 'active');

-- ============================================================================
-- CREDIT BALANCES TABLE
-- ============================================================================

CREATE TABLE credit_balances (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    
    -- Balance (in cents)
    balance_cents BIGINT NOT NULL DEFAULT 0 CHECK (balance_cents >= 0),
    reserved_cents BIGINT NOT NULL DEFAULT 0 CHECK (reserved_cents >= 0),
    available_cents BIGINT NOT NULL DEFAULT 0 CHECK (
        available_cents = balance_cents - reserved_cents AND available_cents >= 0
    ),
    
    -- Currency
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_credit_balances_user_id ON credit_balances(user_id);

-- ============================================================================
-- CREDIT TRANSACTIONS TABLE
-- ============================================================================

CREATE TABLE credit_transactions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    credit_balance_id UUID NOT NULL REFERENCES credit_balances(id) ON DELETE CASCADE,
    
    -- Transaction details
    amount_cents BIGINT NOT NULL, -- Can be negative for deductions
    transaction_type credit_transaction_type NOT NULL,
    description TEXT NOT NULL,
    
    -- Related entities
    invoice_id UUID REFERENCES invoices(id) ON DELETE SET NULL,
    payment_id UUID REFERENCES payments(id) ON DELETE SET NULL,
    
    -- Timestamp
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata
    metadata JSONB
);

CREATE INDEX idx_credit_transactions_user_id ON credit_transactions(user_id);
CREATE INDEX idx_credit_transactions_balance_id ON credit_transactions(credit_balance_id);
CREATE INDEX idx_credit_transactions_type ON credit_transactions(transaction_type);
CREATE INDEX idx_credit_transactions_created_at ON credit_transactions(created_at DESC);
CREATE INDEX idx_credit_transactions_invoice_id ON credit_transactions(invoice_id) WHERE invoice_id IS NOT NULL;

-- ============================================================================
-- PAYMENT METHODS TABLE (Optional - for stored payment methods)
-- ============================================================================

CREATE TABLE payment_methods (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    
    -- Stripe integration
    stripe_payment_method_id VARCHAR(255) NOT NULL UNIQUE,
    
    -- Method details
    type payment_method NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    
    -- Card details (last 4 only for display)
    card_last4 VARCHAR(4),
    card_brand VARCHAR(50),
    card_exp_month INTEGER CHECK (card_exp_month >= 1 AND card_exp_month <= 12),
    card_exp_year INTEGER CHECK (card_exp_year >= 2024),
    
    -- Bank account details (last 4 only)
    bank_last4 VARCHAR(4),
    bank_name VARCHAR(255),
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata
    metadata JSONB
);

CREATE INDEX idx_payment_methods_user_id ON payment_methods(user_id);
CREATE INDEX idx_payment_methods_default ON payment_methods(user_id, is_default) WHERE is_default = TRUE;
CREATE INDEX idx_payment_methods_active ON payment_methods(user_id) WHERE is_active = TRUE;

-- ============================================================================
-- PROMO CODES TABLE (for discounts and promotions)
-- ============================================================================

CREATE TABLE promo_codes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    code VARCHAR(50) NOT NULL UNIQUE,
    
    -- Discount details
    discount_type VARCHAR(20) NOT NULL CHECK (discount_type IN ('percentage', 'fixed_amount')),
    discount_value INTEGER NOT NULL CHECK (discount_value > 0),
    currency VARCHAR(3) DEFAULT 'USD',
    
    -- Restrictions
    max_uses INTEGER CHECK (max_uses IS NULL OR max_uses > 0),
    times_used INTEGER NOT NULL DEFAULT 0 CHECK (times_used >= 0),
    minimum_amount_cents BIGINT CHECK (minimum_amount_cents IS NULL OR minimum_amount_cents > 0),
    
    -- Applicable tiers
    applicable_tiers subscription_tier[],
    
    -- Validity period
    valid_from TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    valid_until TIMESTAMPTZ,
    
    -- Status
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    
    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Metadata
    metadata JSONB,
    
    CONSTRAINT valid_validity_period CHECK (valid_until IS NULL OR valid_until > valid_from),
    CONSTRAINT max_uses_not_exceeded CHECK (max_uses IS NULL OR times_used <= max_uses)
);

CREATE INDEX idx_promo_codes_code ON promo_codes(code);
CREATE INDEX idx_promo_codes_active ON promo_codes(is_active) WHERE is_active = TRUE;
CREATE INDEX idx_promo_codes_valid_until ON promo_codes(valid_until) WHERE valid_until IS NOT NULL;

-- ============================================================================
-- PROMO CODE REDEMPTIONS TABLE
-- ============================================================================

CREATE TABLE promo_code_redemptions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    promo_code_id UUID NOT NULL REFERENCES promo_codes(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    invoice_id UUID REFERENCES invoices(id) ON DELETE SET NULL,
    
    -- Redemption details
    discount_amount_cents BIGINT NOT NULL CHECK (discount_amount_cents > 0),
    
    -- Timestamp
    redeemed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    -- Prevent duplicate redemptions per user
    UNIQUE(promo_code_id, user_id)
);

CREATE INDEX idx_promo_redemptions_code ON promo_code_redemptions(promo_code_id);
CREATE INDEX idx_promo_redemptions_user ON promo_code_redemptions(user_id);
CREATE INDEX idx_promo_redemptions_invoice ON promo_code_redemptions(invoice_id) WHERE invoice_id IS NOT NULL;

-- ============================================================================
-- TRIGGERS
-- ============================================================================

-- Auto-update updated_at timestamp
CREATE OR REPLACE FUNCTION update_billing_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_invoices_updated_at
    BEFORE UPDATE ON invoices
    FOR EACH ROW
    EXECUTE FUNCTION update_billing_updated_at();

CREATE TRIGGER trigger_payments_updated_at
    BEFORE UPDATE ON payments
    FOR EACH ROW
    EXECUTE FUNCTION update_billing_updated_at();

CREATE TRIGGER trigger_subscriptions_updated_at
    BEFORE UPDATE ON subscriptions
    FOR EACH ROW
    EXECUTE FUNCTION update_billing_updated_at();

CREATE TRIGGER trigger_credit_balances_updated_at
    BEFORE UPDATE ON credit_balances
    FOR EACH ROW
    EXECUTE FUNCTION update_billing_updated_at();

CREATE TRIGGER trigger_payment_methods_updated_at
    BEFORE UPDATE ON payment_methods
    FOR EACH ROW
    EXECUTE FUNCTION update_billing_updated_at();

CREATE TRIGGER trigger_promo_codes_updated_at
    BEFORE UPDATE ON promo_codes
    FOR EACH ROW
    EXECUTE FUNCTION update_billing_updated_at();

-- Auto-set processed_at when payment succeeds
CREATE OR REPLACE FUNCTION set_payment_processed_at()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.status = 'succeeded' AND OLD.status != 'succeeded' THEN
        NEW.processed_at = NOW();
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_set_payment_processed_at
    BEFORE UPDATE ON payments
    FOR EACH ROW
    EXECUTE FUNCTION set_payment_processed_at();

-- Enforce only one default payment method per user
CREATE OR REPLACE FUNCTION ensure_single_default_payment_method()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.is_default = TRUE THEN
        UPDATE payment_methods 
        SET is_default = FALSE 
        WHERE user_id = NEW.user_id 
            AND id != NEW.id 
            AND is_default = TRUE;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_ensure_single_default_payment
    AFTER INSERT OR UPDATE ON payment_methods
    FOR EACH ROW
    WHEN (NEW.is_default = TRUE)
    EXECUTE FUNCTION ensure_single_default_payment_method();

-- ============================================================================
-- HELPER FUNCTIONS
-- ============================================================================

-- Get total revenue for a period
CREATE OR REPLACE FUNCTION get_total_revenue(
    start_date TIMESTAMPTZ,
    end_date TIMESTAMPTZ
)
RETURNS BIGINT AS $$
DECLARE
    total BIGINT;
BEGIN
    SELECT COALESCE(SUM(total_cents), 0)
    INTO total
    FROM invoices
    WHERE paid = TRUE 
        AND paid_at >= start_date 
        AND paid_at < end_date;
    
    RETURN total;
END;
$$ LANGUAGE plpgsql;

-- Get user's total outstanding balance
CREATE OR REPLACE FUNCTION get_user_outstanding_balance(p_user_id UUID)
RETURNS BIGINT AS $$
DECLARE
    outstanding BIGINT;
BEGIN
    SELECT COALESCE(SUM(amount_due_cents), 0)
    INTO outstanding
    FROM invoices
    WHERE user_id = p_user_id 
        AND paid = FALSE
        AND status IN ('open', 'past_due');
    
    RETURN outstanding;
END;
$$ LANGUAGE plpgsql;

-- Apply credit to invoice
CREATE OR REPLACE FUNCTION apply_credit_to_invoice(
    p_invoice_id UUID,
    p_user_id UUID
)
RETURNS VOID AS $$
DECLARE
    v_amount_due BIGINT;
    v_available_credit BIGINT;
    v_credit_to_apply BIGINT;
    v_credit_balance_id UUID;
BEGIN
    -- Get invoice amount due
    SELECT amount_due_cents INTO v_amount_due
    FROM invoices
    WHERE id = p_invoice_id AND user_id = p_user_id;
    
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Invoice not found';
    END IF;
    
    -- Get available credit
    SELECT id, available_cents INTO v_credit_balance_id, v_available_credit
    FROM credit_balances
    WHERE user_id = p_user_id;
    
    IF NOT FOUND OR v_available_credit = 0 THEN
        RETURN; -- No credits to apply
    END IF;
    
    -- Calculate credit to apply
    v_credit_to_apply := LEAST(v_amount_due, v_available_credit);
    
    IF v_credit_to_apply > 0 THEN
        -- Deduct from credit balance
        UPDATE credit_balances
        SET balance_cents = balance_cents - v_credit_to_apply,
            available_cents = available_cents - v_credit_to_apply,
            updated_at = NOW()
        WHERE id = v_credit_balance_id;
        
        -- Update invoice
        UPDATE invoices
        SET amount_paid_cents = amount_paid_cents + v_credit_to_apply,
            amount_due_cents = amount_due_cents - v_credit_to_apply,
            discount_cents = discount_cents + v_credit_to_apply,
            updated_at = NOW()
        WHERE id = p_invoice_id;
        
        -- Record credit transaction
        INSERT INTO credit_transactions (
            user_id, credit_balance_id, amount_cents, transaction_type,
            description, invoice_id
        ) VALUES (
            p_user_id, v_credit_balance_id, -v_credit_to_apply, 'applied',
            'Credit applied to invoice', p_invoice_id
        );
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Generate monthly invoice for user
CREATE OR REPLACE FUNCTION generate_monthly_invoice(p_user_id UUID)
RETURNS UUID AS $$
DECLARE
    v_invoice_id UUID;
    v_api_key RECORD;
    v_usage RECORD;
    v_tier subscription_tier;
    v_quota INTEGER;
    v_overage BIGINT;
    v_overage_cost BIGINT;
    v_subscription_cost BIGINT;
    v_period_start TIMESTAMPTZ;
    v_period_end TIMESTAMPTZ;
BEGIN
    -- Calculate billing period (previous month)
    v_period_start := DATE_TRUNC('month', NOW() - INTERVAL '1 month');
    v_period_end := DATE_TRUNC('month', NOW());
    
    -- Create invoice
    INSERT INTO invoices (
        user_id, subtotal_cents, tax_cents, discount_cents,
        total_cents, amount_due_cents, currency, invoice_number,
        description, billing_period_start, billing_period_end,
        due_date, status
    )
    SELECT 
        p_user_id,
        0, -- Will be calculated
        0,
        0,
        0,
        0,
        'USD',
        'INV-' || TO_CHAR(NOW(), 'YYYYMM') || '-' || LPAD((SELECT COUNT(*) + 1 FROM invoices)::TEXT, 6, '0'),
        'Monthly usage invoice for ' || TO_CHAR(v_period_start, 'Month YYYY'),
        v_period_start,
        v_period_end,
        v_period_end + INTERVAL '15 days',
        'open'
    RETURNING id INTO v_invoice_id;
    
    -- Add line items for each API key
    FOR v_api_key IN 
        SELECT * FROM api_keys WHERE user_id = p_user_id AND is_active = TRUE
    LOOP
        v_tier := v_api_key.tier;
        v_quota := v_api_key.monthly_message_quota;
        
        -- Get usage
        SELECT COALESCE(SUM(total_messages_sent), 0) INTO v_overage
        FROM usage_metrics_daily
        WHERE api_key_id = v_api_key.id
            AND day >= v_period_start
            AND day < v_period_end;
        
        v_overage := GREATEST(v_overage - v_quota, 0);
        
        -- Calculate overage cost ($0.01 per message)
        v_overage_cost := v_overage * 10; -- 10 cents per 10 messages = 1 cent per message
        
        IF v_overage_cost > 0 THEN
            INSERT INTO invoice_line_items (
                invoice_id, api_key_id, description, item_type,
                quantity, unit_price_cents, amount_cents
            ) VALUES (
                v_invoice_id, v_api_key.id, 
                'Message overage (' || v_overage || ' extra messages)',
                'message_overage', v_overage, 1, v_overage_cost
            );
        END IF;
    END LOOP;
    
    -- Update invoice total
    UPDATE invoices
    SET subtotal_cents = (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_line_items WHERE invoice_id = v_invoice_id),
        total_cents = (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_line_items WHERE invoice_id = v_invoice_id),
        amount_due_cents = (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_line_items WHERE invoice_id = v_invoice_id)
    WHERE id = v_invoice_id;
    
    RETURN v_invoice_id;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- VIEWS FOR REPORTING
-- ============================================================================

-- Monthly revenue report
CREATE OR REPLACE VIEW monthly_revenue AS
SELECT 
    DATE_TRUNC('month', paid_at) as month,
    COUNT(*) as invoice_count,
    SUM(total_cents) as total_revenue_cents,
    SUM(amount_paid_cents) as paid_revenue_cents,
    AVG(total_cents) as avg_invoice_cents
FROM invoices
WHERE paid = TRUE
GROUP BY DATE_TRUNC('month', paid_at)
ORDER BY month DESC;

-- User billing summary
CREATE OR REPLACE VIEW user_billing_summary AS
SELECT 
    u.id as user_id,
    u.email,
    COUNT(DISTINCT i.id) as total_invoices,
    COUNT(DISTINCT i.id) FILTER (WHERE i.paid = TRUE) as paid_invoices,
    COUNT(DISTINCT i.id) FILTER (WHERE i.paid = FALSE) as unpaid_invoices,
    COALESCE(SUM(i.total_cents) FILTER (WHERE i.paid = TRUE), 0) as lifetime_revenue_cents,
    COALESCE(SUM(i.amount_due_cents) FILTER (WHERE i.paid = FALSE), 0) as outstanding_balance_cents,
    COALESCE(cb.available_cents, 0) as credit_balance_cents
FROM users u
LEFT JOIN invoices i ON u.id = i.user_id
LEFT JOIN credit_balances cb ON u.id = cb.user_id
GROUP BY u.id, u.email, cb.available_cents;

-- ============================================================================
-- COMMENTS
-- ============================================================================

COMMENT ON TABLE invoices IS 'Billing invoices for subscription and usage charges';
COMMENT ON TABLE invoice_line_items IS 'Itemized charges within an invoice';
COMMENT ON TABLE payments IS 'Payment transactions for invoices';
COMMENT ON TABLE subscriptions IS 'User subscription management (Stripe-backed)';
COMMENT ON TABLE credit_balances IS 'User credit balances for prepaid credits';
COMMENT ON TABLE credit_transactions IS 'Audit trail for credit additions and deductions';
COMMENT ON TABLE payment_methods IS 'Stored payment methods for users';
COMMENT ON TABLE promo_codes IS 'Promotional discount codes';
COMMENT ON TABLE promo_code_redemptions IS 'Tracking of promo code usage';

COMMENT ON COLUMN invoices.amount_due_cents IS 'Total amount still owed (total - amount_paid)';
COMMENT ON COLUMN credit_balances.reserved_cents IS 'Credits reserved for pending invoices';
COMMENT ON COLUMN credit_balances.available_cents IS 'Credits available for use (balance - reserved)';
COMMENT ON COLUMN promo_codes.discount_type IS 'Either percentage (e.g., 20 for 20%) or fixed_amount (in cents)';
