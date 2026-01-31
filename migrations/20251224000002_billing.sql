-- =============================================================================
-- Billing Periods (Authoritative)
-- =============================================================================

CREATE TABLE public.billing_periods (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    application_id uuid NOT NULL,
    developer_id uuid NOT NULL,

    period_start timestamptz NOT NULL,
    period_end timestamptz NOT NULL,

    status text NOT NULL
        CHECK (status IN ('open', 'closed', 'invoiced')),

    created_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT billing_period_valid CHECK (period_end > period_start),

    CONSTRAINT billing_period_app_fkey
        FOREIGN KEY (application_id)
        REFERENCES applications(id)
        ON DELETE CASCADE,

    CONSTRAINT unique_billing_period
        UNIQUE (application_id, period_start)
);

-- =============================================================================
-- Client Billing Usage (Frozen Usage Snapshot)
-- =============================================================================

CREATE TABLE public.client_billing_usage (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    billing_period_id uuid NOT NULL,
    client_id uuid NOT NULL,
    application_id uuid NOT NULL,

    -- frozen usage
    messages_sent bigint NOT NULL,
    messages_received bigint NOT NULL,
    proofs_verified bigint NOT NULL,
    total_bytes_stored bigint NOT NULL,
    total_bytes_sent bigint NOT NULL,
    total_bytes_received bigint NOT NULL,
    rate_limit_hits integer NOT NULL,

    developer_id uuid NOT NULL,
    revenue_snapshot jsonb NOT NULL,

    created_at timestamptz NOT NULL DEFAULT now(),

    -- Constraints
    CONSTRAINT billing_usage_developer_fkey
        FOREIGN KEY (developer_id)
        REFERENCES users(id)
        ON DELETE CASCADE,

    CONSTRAINT billing_usage_period_fkey
        FOREIGN KEY (billing_period_id)
        REFERENCES billing_periods(id)
        ON DELETE CASCADE,

    CONSTRAINT billing_usage_client_fkey
        FOREIGN KEY (client_id)
        REFERENCES clients(id)
        ON DELETE CASCADE
);

-- =============================================================================
-- Client Invoices
-- =============================================================================

CREATE TABLE public.client_invoices (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    billing_period_id uuid NOT NULL,
    client_id uuid NOT NULL,
    application_id uuid NOT NULL,

    developer_id uuid NOT NULL,

    pricing_snapshot jsonb NOT NULL,

    subtotal_cents bigint NOT NULL,
    total_cents bigint NOT NULL,

    status text NOT NULL
        CHECK (status IN ('pending', 'finalized', 'paid', 'failed')),

    created_at timestamptz NOT NULL DEFAULT now(),

    -- Constraints
    CONSTRAINT client_invoices_developer_fkey
        FOREIGN KEY (developer_id)
        REFERENCES users(id)
        ON DELETE CASCADE,

    CONSTRAINT invoice_period_fkey
        FOREIGN KEY (billing_period_id)
        REFERENCES billing_periods(id)
        ON DELETE CASCADE,

    CONSTRAINT unique_client_invoice_period
        UNIQUE (client_id, billing_period_id)
);