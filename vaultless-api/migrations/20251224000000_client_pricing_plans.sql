-- =============================================================================
-- Client Pricing Plans
-- Developer-defined pricing for billing clients under an application
-- =============================================================================

CREATE TABLE IF NOT EXISTS public.client_pricing_plans (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    application_id uuid NOT NULL,
    name text NOT NULL,

    -- pricing mode
    pricing_mode text NOT NULL
        CHECK (pricing_mode IN ('fixed', 'usage')),

    -- usage-based pricing (nullable, cents)
    price_per_message_cents bigint,
    price_per_gb_cents bigint,
    price_per_proof_cents bigint,

    -- fixed pricing (cents)
    fixed_price_cents bigint,

    created_at timestamptz NOT NULL DEFAULT now(),

    -- Ensure valid pricing configuration
    CONSTRAINT pricing_plan_valid CHECK (
        (pricing_mode = 'fixed' AND fixed_price_cents IS NOT NULL)
        OR
        (pricing_mode = 'usage' AND (
            price_per_message_cents IS NOT NULL
            OR price_per_gb_cents IS NOT NULL
            OR price_per_proof_cents IS NOT NULL
        ))
    ),

    -- Application ownership
    CONSTRAINT client_pricing_plans_application_fkey
        FOREIGN KEY (application_id)
        REFERENCES public.applications(id)
        ON DELETE CASCADE
);

ALTER TABLE public.client_pricing_plans
    OWNER TO vaultless;

-- =============================================================================
-- Indexes
-- =============================================================================

-- Primary access pattern: list pricing plans per application
CREATE INDEX IF NOT EXISTS idx_client_pricing_plans_application
ON public.client_pricing_plans (application_id);

-- Prevent duplicate pricing plan names within the same application
CREATE UNIQUE INDEX IF NOT EXISTS idx_client_pricing_plans_app_name
ON public.client_pricing_plans (application_id, name);
