-- Add migration script here

-- 1. Define the Generic Utility Function (MUST be defined first)
----------------------------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION public.update_updated_at()
    RETURNS TRIGGER AS
$$
BEGIN
    -- Ensure the 'updated_at' column is set to the current time
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;


-- 2. Create the Webhooks Table
----------------------------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.webhooks
(
    -- Primary Key
    id uuid NOT NULL DEFAULT uuid_generate_v4(),
    
    -- Foreign Key to the parent application
    application_id uuid NOT NULL,

    -- The destination URL where the webhook payload will be sent
    url text COLLATE pg_catalog."default" NOT NULL,

    -- The type of event that triggers this webhook (e.g., KEY_EXPIRED, QUOTA_EXCEEDED)
    event_type character varying(100) COLLATE pg_catalog."default" NOT NULL,

    -- A secret used to sign the webhook payload for verification by the recipient
    signing_secret character varying(255) COLLATE pg_catalog."default" NOT NULL,

    -- Whether this specific webhook subscription is active or paused
    is_active boolean NOT NULL DEFAULT true,

    -- Tracking information
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),

    -- Constraints
    CONSTRAINT webhooks_pkey PRIMARY KEY (id),
    
    -- Enforce that the URL for a specific event type is unique per application
    CONSTRAINT webhooks_app_url_type_unique UNIQUE (application_id, url, event_type),

    -- Foreign Key Constraint to the applications table
    CONSTRAINT webhooks_application_id_fkey FOREIGN KEY (application_id)
        REFERENCES public.applications (id) MATCH SIMPLE
        ON UPDATE NO ACTION
        ON DELETE CASCADE
)

TABLESPACE pg_default;

ALTER TABLE public.webhooks
    OWNER to vaultless;


-- 3. Add Indexes
----------------------------------------------------------------------------------------------------
-- Index: public.idx_webhooks_application_id
CREATE INDEX IF NOT EXISTS idx_webhooks_application_id
    ON public.webhooks USING btree
    (application_id ASC NULLS LAST)
    TABLESPACE pg_default;


-- 4. Define/Redefine Triggers to use the generic function
----------------------------------------------------------------------------------------------------
-- Trigger for the 'applications' table
DROP TRIGGER IF EXISTS trigger_applications_updated_at ON public.applications;
CREATE OR REPLACE TRIGGER trigger_applications_updated_at
    BEFORE UPDATE 
    ON public.applications
    FOR EACH ROW
    EXECUTE FUNCTION public.update_updated_at();

-- Trigger for the new 'webhooks' table
-- NOTE: The original script had a temporary function name here that was removed.
DROP TRIGGER IF EXISTS trigger_webhooks_updated_at ON public.webhooks;
CREATE OR REPLACE TRIGGER trigger_webhooks_updated_at
    BEFORE UPDATE 
    ON public.webhooks
    FOR EACH ROW
    -- CORRECT CALL: Use the generic function
    EXECUTE FUNCTION public.update_updated_at();