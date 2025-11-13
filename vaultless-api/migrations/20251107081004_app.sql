-- ============================================================================
-- Description: Implements pk_/sk_ key system for developers
-- ============================================================================

BEGIN;

-- Step 1: Create applications table
CREATE TABLE IF NOT EXISTS public.applications
(
    id uuid NOT NULL DEFAULT uuid_generate_v4(),
    user_id uuid NOT NULL,
    name character varying(255) COLLATE pg_catalog."default" NOT NULL,
    description text COLLATE pg_catalog."default",
    
    -- The secret key (references existing api_keys table)
    api_key_id uuid NOT NULL,
    
    -- Application metadata
    bundle_id character varying(255) COLLATE pg_catalog."default",
    platform character varying(50) COLLATE pg_catalog."default",
    webhook_url text COLLATE pg_catalog."default",
    
    -- Status and timestamps
    is_active boolean NOT NULL DEFAULT true,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    
    -- Constraints
    CONSTRAINT applications_pkey PRIMARY KEY (id),
    CONSTRAINT applications_user_id_fkey FOREIGN KEY (user_id)
        REFERENCES public.users (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE CASCADE,
    CONSTRAINT applications_api_key_id_fkey FOREIGN KEY (api_key_id)
        REFERENCES public.api_keys (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE CASCADE,
    CONSTRAINT valid_name CHECK (char_length(name) > 0)
)
TABLESPACE pg_default;

ALTER TABLE public.applications OWNER to vaultless;

-- Indexes for applications table
CREATE INDEX idx_applications_user_id 
    ON public.applications USING btree (user_id ASC NULLS LAST)
    TABLESPACE pg_default;

CREATE INDEX idx_applications_api_key_id 
    ON public.applications USING btree (api_key_id ASC NULLS LAST)
    TABLESPACE pg_default;

CREATE INDEX idx_applications_active 
    ON public.applications USING btree (is_active ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE is_active = true;

CREATE INDEX idx_applications_bundle_id 
    ON public.applications USING btree (bundle_id COLLATE pg_catalog."default" ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE bundle_id IS NOT NULL;

-- Step 2: Add updated_at trigger for applications
CREATE OR REPLACE FUNCTION public.update_applications_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_applications_updated_at
    BEFORE UPDATE ON public.applications
    FOR EACH ROW
    EXECUTE FUNCTION public.update_applications_updated_at();

-- Step 3: Add application_id to clients table
ALTER TABLE public.clients 
ADD COLUMN IF NOT EXISTS application_id uuid;

-- Add foreign key constraint
ALTER TABLE public.clients
ADD CONSTRAINT clients_application_id_fkey 
FOREIGN KEY (application_id)
REFERENCES public.applications (id)
ON UPDATE NO ACTION
ON DELETE SET NULL;

-- Add index for application_id lookups (already exists in your schema, but adding for safety)
CREATE INDEX IF NOT EXISTS idx_clients_application_id 
    ON public.clients USING btree (application_id ASC NULLS LAST)
    TABLESPACE pg_default;

-- Composite index for common queries (app + active clients)
CREATE INDEX IF NOT EXISTS idx_clients_app_active
    ON public.clients USING btree (application_id ASC NULLS LAST, is_active ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE application_id IS NOT NULL AND is_active = true;

-- Step 5: Add comments for documentation
COMMENT ON COLUMN public.applications.api_key_id IS 
'References the secret API key (sk_) used for billing, metrics, and rate limiting.';

COMMENT ON COLUMN public.applications.bundle_id IS 
'iOS/Android bundle identifier for app store verification (e.g., com.example.app)';

COMMENT ON COLUMN public.applications.platform IS 
'Target platform: web, ios, android, desktop, or other';

COMMENT ON COLUMN public.clients.application_id IS 
'Links client to the application they registered through. Enables per-app analytics and client management.';

-- Step 6: Create helper view for common queries
CREATE OR REPLACE VIEW public.v_client_applications AS
SELECT 
    c.id as client_id,
    c.identifier,
    c.public_key,
    c.is_active as client_active,
    c.created_at as client_created_at,
    c.last_seen_at,
    c.last_message_at,
    a.id as application_id,
    a.name as application_name,
    a.platform,
    a.is_active as application_active,
    ak.id as api_key_id,
    ak.tier,
    ak.monthly_message_quota,
    ak.rate_limit_per_minute,
    u.id as developer_id,
    u.email as developer_email
FROM public.clients c
LEFT JOIN public.applications a ON c.application_id = a.id
LEFT JOIN public.api_keys ak ON a.api_key_id = ak.id
LEFT JOIN public.users u ON a.user_id = u.id;

COMMENT ON VIEW public.v_client_applications IS 
'Convenience view joining clients with their application, API key, and developer details.';

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



