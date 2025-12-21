-- ============================================================================
-- Description: Implements pk_/sk_ key system for developers
-- ============================================================================
 BEGIN;

-- Step 1: Create applications table

CREATE TABLE IF NOT EXISTS public.applications
    (id uuid NOT NULL DEFAULT uuid_generate_v4(),
                              user_id uuid NOT NULL,
                                           subscription_id uuid NOT NULL REFERENCES subscriptions(id),
                                                                                    name character varying(255) COLLATE pg_catalog."default" NOT NULL,
                                                                                                                                             description text COLLATE pg_catalog."default", -- Status and timestamps
 is_active boolean NOT NULL DEFAULT true,
                                    created_at timestamp with time zone NOT NULL DEFAULT now(),
                                                                                         updated_at timestamp with time zone NOT NULL DEFAULT now(), -- Constraints
 CONSTRAINT applications_pkey PRIMARY KEY (id), CONSTRAINT applications_user_id_fkey
     FOREIGN KEY (user_id) REFERENCES public.users (id) MATCH SIMPLE ON UPDATE NO ACTION ON DELETE CASCADE,
                                                                                                   CONSTRAINT valid_name CHECK (char_length(name) > 0)) TABLESPACE pg_default;


ALTER TABLE public.applications OWNER to vaultless;

-- Indexes for applications table

CREATE INDEX idx_applications_user_id ON public.applications USING btree (user_id ASC NULLS LAST) TABLESPACE pg_default;


CREATE INDEX idx_applications_subscription_id ON public.applications USING btree (subscription_id ASC NULLS LAST);


CREATE INDEX idx_applications_active ON public.applications USING btree (is_active ASC NULLS LAST) TABLESPACE pg_default
WHERE is_active = true;

-- Step 2: Add updated_at trigger for applications

CREATE OR REPLACE FUNCTION public.update_applications_updated_at() RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;


CREATE TRIGGER trigger_applications_updated_at
BEFORE
UPDATE ON public.applications
FOR EACH ROW EXECUTE FUNCTION public.update_applications_updated_at();

-- Step 3: Add application_id to clients table

ALTER TABLE public.clients ADD COLUMN IF NOT EXISTS application_id uuid;

-- Add foreign key constraint

ALTER TABLE public.clients ADD CONSTRAINT clients_application_id_fkey
FOREIGN KEY (application_id) REFERENCES public.applications (id) ON
UPDATE NO ACTION ON
DELETE
SET NULL;

-- Add index for application_id lookups (already exists in your schema, but adding for safety)

CREATE INDEX IF NOT EXISTS idx_clients_application_id ON public.clients USING btree (application_id ASC NULLS LAST) TABLESPACE pg_default;

-- Composite index for common queries (app + active clients)

CREATE INDEX IF NOT EXISTS idx_clients_app_active ON public.clients USING btree (application_id ASC NULLS LAST, is_active ASC NULLS LAST) TABLESPACE pg_default
WHERE application_id IS NOT NULL
    AND is_active = true;

COMMENT ON COLUMN public.clients.application_id IS 'Links client to the application they registered through. Enables per-app analytics and client management.';


COMMIT;

-- ============================================================================
-- Verification Queries (Run these to verify migration success)
-- ============================================================================
 -- Verify applications table
-- SELECT COUNT(*) FROM public.applications;
 -- Verify clients.application_id column exists
-- SELECT column_name, data_type FROM information_schema.columns
-- WHERE table_name = 'clients' AND column_name = 'application_id';
 -- Verify indexes
-- SELECT indexname FROM pg_indexes
-- WHERE tablename IN ('applications', 'clients')
-- ORDER BY tablename, indexname;
 -- Verify key_type enum
-- SELECT enumlabel FROM pg_enum WHERE enumtypid = 'key_type'::regtype;
