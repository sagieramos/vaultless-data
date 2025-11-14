-- Add migration script here
ALTER TABLE public.applications
    -- Security & Configuration
    ADD COLUMN max_ttl_seconds integer NOT NULL DEFAULT 604800,
    ADD COLUMN is_key_rotation_forced boolean NOT NULL DEFAULT false,
    
    -- Auditing & Metrics
    ADD COLUMN deletion_requested_at timestamp with time zone,
    ADD COLUMN internal_notes text,

    -- dd the new, flexible JSONB column for all platform-specific security configuration
    ADD COLUMN integrity_config jsonb NOT NULL DEFAULT '{}'::jsonb;


    -- Add an index to efficiently check which apps are due for key rotation
CREATE INDEX IF NOT EXISTS idx_applications_rotation_check
    ON public.applications (is_key_rotation_forced, updated_at)
    TABLESPACE pg_default
    WHERE is_key_rotation_forced = true;
    
-- Add an index for quick lookup of apps awaiting deletion
CREATE INDEX IF NOT EXISTS idx_applications_deletion_requested
    ON public.applications (deletion_requested_at)
    TABLESPACE pg_default
    WHERE deletion_requested_at IS NOT NULL;

-- Create GIN index for efficient querying within the JSONB column
CREATE INDEX IF NOT EXISTS idx_applications_integrity_config_gin
    ON public.applications USING GIN (integrity_config);

ALTER TABLE public.clients
ADD COLUMN IF NOT EXISTS is_platform_attested boolean NOT NULL DEFAULT false;

-- Optional: Add an index for faster lookups on active/attested clients
CREATE INDEX IF NOT EXISTS idx_clients_attested
    ON public.clients USING btree
    (application_id ASC NULLS LAST, is_platform_attested ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE is_platform_attested = true;