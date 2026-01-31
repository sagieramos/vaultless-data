-- Migration: Create process_usage_event function for atomic billing operations

-- This function implements the core billing logic atomically:
-- 1. Checks if client has sufficient credits
-- 2. Deducts required credits from client
-- 3. Records the credit transaction
-- 4. Calculates and records revenue share for developer
-- 5. Returns remaining credits

CREATE OR REPLACE FUNCTION process_usage_event(
    p_client_id UUID,
    p_application_id UUID,
    p_developer_id UUID,
    p_pricing_plan_id UUID,  -- Using pricing plan ID directly instead of snapshot
    p_messages_sent BIGINT,
    p_messages_received BIGINT,
    p_bytes_sent BIGINT,
    p_bytes_received BIGINT,
    p_proofs_verified BIGINT,
    p_billing_period_id UUID
) RETURNS BIGINT AS $$
DECLARE
    v_total_messages BIGINT;
    v_total_bytes BIGINT;
    v_required_credits BIGINT;
    v_client_credit BIGINT;
    v_gross_revenue_cents BIGINT;
    v_platform_fee_percent NUMERIC(5,2) := 10.00;  -- Default 10.00% platform fee
    v_platform_fee_cents BIGINT;
    v_net_revenue_cents BIGINT;
    v_remaining_credits BIGINT;
    v_price_per_message_cents BIGINT;
    v_price_per_gb_cents BIGINT;
    v_price_per_proof_cents BIGINT;
BEGIN
    -- Calculate usage totals
    v_total_messages := p_messages_sent + p_messages_received;
    v_total_bytes := p_bytes_sent + p_bytes_received;

    -- Get pricing information from the pricing plan
    SELECT
        COALESCE(price_per_message_cents, 0),
        COALESCE(price_per_gb_cents, 0),
        COALESCE(price_per_proof_cents, 0)
    INTO
        v_price_per_message_cents,
        v_price_per_gb_cents,
        v_price_per_proof_cents
    FROM pricing_plans
    WHERE id = p_pricing_plan_id;

    -- Calculate required credits based on usage and pricing (these are non-cash usage units)
    v_required_credits :=
        (v_total_messages * v_price_per_message_cents) +
        ((v_total_bytes / (1024*1024*1024)) * v_price_per_gb_cents) +  -- Convert bytes to GB
        (p_proofs_verified * v_price_per_proof_cents);

    -- Lock and get the client's current credit balance
    SELECT credit_balance
    INTO v_client_credit
    FROM client_usage_credits
    WHERE client_id = p_client_id
    FOR UPDATE;

    -- Check if client has sufficient credits
    IF v_client_credit < v_required_credits THEN
        RAISE EXCEPTION 'Insufficient credits for this usage';
    END IF;

    -- Update the client's credit balance (deduct required credits)
    UPDATE client_usage_credits
    SET
        credit_balance = credit_balance - v_required_credits,
        credit_consumed = credit_consumed + v_required_credits,
        updated_at = NOW()
    WHERE client_id = p_client_id;

    -- Record the credit transaction
    INSERT INTO credit_transactions (
        client_id,
        application_id,
        transaction_type,
        amount,
        usage_context,
        related_transaction_id,
        billing_period_id
    )
    VALUES (
        p_client_id,
        p_application_id,
        'usage_deduction',
        -v_required_credits,  -- Negative to indicate deduction
        json_build_object(
            'messages_sent', p_messages_sent,
            'messages_received', p_messages_received,
            'bytes_sent', p_bytes_sent,
            'bytes_received', p_bytes_received,
            'proofs_verified', p_proofs_verified,
            'pricing_snapshot_id', p_pricing_snapshot_id
        ),
        NULL,  -- No related transaction for usage deduction
        p_billing_period_id
    );

    -- Calculate gross revenue in cents based on usage
    -- This is accounting metadata only, not real money held by platform
    v_gross_revenue_cents :=
        (v_total_messages * v_price_per_message_cents) +
        ((v_total_bytes / (1024*1024*1024)) * v_price_per_gb_cents) +  -- Convert bytes to GB
        (p_proofs_verified * v_price_per_proof_cents);

    -- Platform takes a percentage (configurable per application or globally)
    v_platform_fee_cents := (v_gross_revenue_cents * v_platform_fee_percent / 100)::BIGINT;
    v_net_revenue_cents := v_gross_revenue_cents - v_platform_fee_cents;

    -- Create revenue share record (this is accounting metadata, not real money held by platform)
    INSERT INTO developer_revenue_shares (
        developer_id,
        application_id,
        billing_period_id,
        messages_processed,
        bytes_transferred,
        proofs_verified,
        usage_value_cents,
        platform_fee_percent,
        platform_fee_cents,
        net_usage_value_cents,
        settlement_currency
    )
    VALUES (
        p_developer_id,
        p_application_id,
        p_billing_period_id,
        v_total_messages,
        v_total_bytes,
        p_proofs_verified,
        v_gross_revenue_cents,
        v_platform_fee_percent,
        v_platform_fee_cents,
        v_net_revenue_cents,
        'USD'  -- settlement_currency - would come from application/developer settings in real implementation
    );

    -- Get the remaining credits after the deduction
    SELECT credit_balance
    INTO v_remaining_credits
    FROM client_usage_credits
    WHERE client_id = p_client_id;

    RETURN v_remaining_credits;
END;
$$ LANGUAGE plpgsql;