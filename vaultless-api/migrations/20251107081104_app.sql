-- ============================================================================
-- Description: Implements pk_/sk_ key system for developers
-- ============================================================================

BEGIN;

-- Step 1: Create applications table
CREATE TABLE IF NOT EXISTS public.applications (
    id uuid NOT NULL DEFAULT uuid_generate_v4(),
    developer_id uuid NOT NULL,
    subscription_id uuid REFERENCES developer_subscriptions(id),
    name character varying(255) NOT NULL,
    description text,
    -- Status and timestamps
    is_active boolean NOT NULL DEFAULT true,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    -- Constraints
    CONSTRAINT applications_pkey PRIMARY KEY (id),
    CONSTRAINT applications_developer_id_fkey
        FOREIGN KEY (developer_id) REFERENCES public.users(id)
        ON UPDATE NO ACTION ON DELETE CASCADE,
    CONSTRAINT valid_name CHECK (char_length(name) > 0)
) TABLESPACE pg_default;

ALTER TABLE public.applications OWNER TO vaultless;

-- Indexes
CREATE INDEX idx_applications_developer_id
    ON public.applications USING btree (developer_id ASC NULLS LAST);

CREATE INDEX idx_applications_subscription_id
    ON public.applications USING btree (subscription_id ASC NULLS LAST);

CREATE INDEX idx_applications_active
    ON public.applications USING btree (is_active ASC NULLS LAST)
    WHERE is_active = true;

-- Trigger to update updated_at timestamp
CREATE OR REPLACE FUNCTION public.update_applications_updated_at()
RETURNS trigger AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_applications_updated_at
    BEFORE UPDATE ON public.applications
    FOR EACH ROW
    EXECUTE FUNCTION public.update_applications_updated_at();

-- Step 2: Add application_id to clients table
ALTER TABLE public.clients
    ADD COLUMN IF NOT EXISTS application_id uuid;

ALTER TABLE public.clients
    ADD CONSTRAINT clients_application_id_fkey
        FOREIGN KEY (application_id) REFERENCES public.applications(id)
        ON UPDATE NO ACTION ON DELETE SET NULL;

-- Indexes on clients
CREATE INDEX IF NOT EXISTS idx_clients_application_id
    ON public.clients USING btree (application_id ASC NULLS LAST);

CREATE INDEX IF NOT EXISTS idx_clients_app_active
    ON public.clients USING btree (application_id ASC NULLS LAST, is_active ASC NULLS LAST)
    WHERE application_id IS NOT NULL AND is_active = true;

-- Comment
COMMENT ON COLUMN public.clients.application_id
    IS 'Links client to the application they registered through. Enables per-app analytics and client management.';

COMMIT;