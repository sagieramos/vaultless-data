CREATE TABLE IF NOT EXISTS public.subscriptions (
    id uuid NOT NULL DEFAULT uuid_generate_v4(),
    user_id uuid NOT NULL,
    
    -- Plan details
    tier subscription_tier NOT NULL DEFAULT 'free'::subscription_tier,
    monthly_message_quota bigint NOT NULL DEFAULT 1000,
    message_retention_seconds bigint NOT NULL DEFAULT 604800,
    rate_limit_per_minute integer NOT NULL DEFAULT 60,
    
    -- Billing cycle management
    is_active boolean NOT NULL DEFAULT true,
    current_period_start timestamp with time zone NOT NULL DEFAULT now(),
    current_period_end timestamp with time zone,
    
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),

    CONSTRAINT subscriptions_pkey PRIMARY KEY (id),
    CONSTRAINT subscriptions_application_id_fkey FOREIGN KEY (application_id)
        REFERENCES public.applications (id) ON DELETE CASCADE,
    CONSTRAINT subscriptions_user_id_fkey FOREIGN KEY (user_id)
        REFERENCES public.users (id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user_active
ON public.subscriptions (user_id)
WHERE is_active = true;

