
-- =============================================================================
-- Pricing Mode Enum
-- =============================================================================

CREATE TYPE pricing_mode_enum AS ENUM (
    'postpaid',
    'prepaid',
    'free'
);

-- =============================================================================
-- Subscription Status Enum
-- =============================================================================

CREATE TYPE subscription_status_enum AS ENUM (
    'active',
    'paused',
    'cancelled'
);

-- =============================================================================
-- Pricing Plans
-- =============================================================================

CREATE TABLE public.pricing_plans (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    developer_id uuid NOT NULL,
    name text NOT NULL,

    pricing_mode pricing_mode_enum NOT NULL,

    -- usage pricing (cents)
    price_per_message_cents bigint,
    price_per_gb_cents bigint,
    price_per_proof_cents bigint,

    -- prepaid pricing (cents)
    prepaid_amount_cents bigint,

    created_at timestamptz NOT NULL DEFAULT now(),

    -- Pricing rules by mode
    CONSTRAINT pricing_plan_valid CHECK (
        -- FREE
        (pricing_mode = 'free'
            AND price_per_message_cents IS NULL
            AND price_per_gb_cents IS NULL
            AND price_per_proof_cents IS NULL
            AND prepaid_amount_cents IS NULL
        )

        OR

        -- POSTPAID
        (pricing_mode = 'postpaid'
            AND (
                price_per_message_cents IS NOT NULL
                OR price_per_gb_cents IS NOT NULL
                OR price_per_proof_cents IS NOT NULL
            )
            AND prepaid_amount_cents IS NULL
        )

        OR

        -- PREPAID
        (pricing_mode = 'prepaid'
            AND prepaid_amount_cents IS NOT NULL
        )
    ),

    CONSTRAINT pricing_plans_developer_fkey
        FOREIGN KEY (developer_id)
        REFERENCES users(id)
        ON DELETE CASCADE
);


-- =============================================================================
-- Application Pricing Plans
-- Which plans an application offers
-- =============================================================================

CREATE TABLE public.application_pricing_plans (
    application_id uuid NOT NULL,
    pricing_plan_id uuid NOT NULL,

    is_default boolean NOT NULL DEFAULT false,
    attached_at timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (application_id, pricing_plan_id),

    CONSTRAINT app_pricing_plans_application_fkey
        FOREIGN KEY (application_id)
        REFERENCES applications(id)
        ON DELETE CASCADE,

    CONSTRAINT app_pricing_plans_plan_fkey
        FOREIGN KEY (pricing_plan_id)
        REFERENCES pricing_plans(id)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_one_default_pricing_plan_per_app
ON public.application_pricing_plans (application_id)
WHERE is_default = true;


-- =============================================================================
-- Client Application Subscriptions
-- =============================================================================

CREATE TABLE public.client_subscriptions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    client_id uuid NOT NULL,
    application_id uuid NOT NULL,
    pricing_plan_id uuid NOT NULL,

    status subscription_status_enum NOT NULL DEFAULT 'active',

    started_at timestamptz NOT NULL DEFAULT now(),
    ended_at timestamptz,

    -- immutable pricing snapshot for billing
    pricing_snapshot jsonb NOT NULL,

    CONSTRAINT client_subscriptions_client_fkey
        FOREIGN KEY (client_id)
        REFERENCES clients(id)
        ON DELETE CASCADE,

    CONSTRAINT client_subscriptions_application_fkey
        FOREIGN KEY (application_id)
        REFERENCES applications(id)
        ON DELETE CASCADE,

    -- 🔒 Enforce that the plan belongs to the application
    CONSTRAINT client_subscriptions_plan_app_fkey
        FOREIGN KEY (application_id, pricing_plan_id)
        REFERENCES application_pricing_plans (application_id, pricing_plan_id)
);

CREATE UNIQUE INDEX idx_unique_active_client_app
ON public.client_subscriptions (client_id, application_id)
WHERE status = 'active';



