-- Add application_id to developer_subscriptions to associate subscriptions with specific applications
ALTER TABLE public.developer_subscriptions
    ADD COLUMN application_id uuid;

-- Add foreign key constraint
ALTER TABLE public.developer_subscriptions
    ADD CONSTRAINT developer_subscriptions_application_id_fkey
    FOREIGN KEY (application_id) REFERENCES public.applications (id) ON DELETE CASCADE;

-- Add index for lookups by application_id
CREATE INDEX IF NOT EXISTS idx_developer_subscriptions_application_id
ON public.developer_subscriptions (application_id)
WHERE is_active = true;
