-- =================================================================================================
-- MIGRATION: Webhooks Table + Updated Timestamp Trigger
-- Description:
--   - Create generic updated_at trigger function
--   - Create "webhooks" table
--   - Add indexes
--   - Add BEFORE UPDATE triggers for updated_at tracking
-- =================================================================================================

----------------------------------------------------------------------------------------------------
-- 1. Generic Utility Function (must exist before triggers)
----------------------------------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION public.update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

----------------------------------------------------------------------------------------------------
-- 2. Webhooks Table
----------------------------------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS public.webhooks (
    id              uuid PRIMARY KEY DEFAULT uuid_generate_v4(),

    application_id  uuid NOT NULL,

    url             text NOT NULL,
    event_type      varchar(100) NOT NULL,
    signing_secret  varchar(255) NOT NULL,

    is_active       boolean NOT NULL DEFAULT TRUE,

    created_at      timestamptz NOT NULL DEFAULT NOW(),
    updated_at      timestamptz NOT NULL DEFAULT NOW(),

    CONSTRAINT webhooks_app_url_type_unique
        UNIQUE (application_id, url, event_type),

    CONSTRAINT webhooks_application_id_fkey
        FOREIGN KEY (application_id)
        REFERENCES public.applications(id)
        ON DELETE CASCADE
);

ALTER TABLE public.webhooks OWNER TO vaultless;

----------------------------------------------------------------------------------------------------
-- 3. Indexes
----------------------------------------------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_webhooks_application_id
    ON public.webhooks (application_id);

----------------------------------------------------------------------------------------------------
-- 4. BEFORE UPDATE triggers for updated_at
----------------------------------------------------------------------------------------------------

-- Applications table
DROP TRIGGER IF EXISTS trigger_applications_updated_at ON public.applications;
CREATE TRIGGER trigger_applications_updated_at
    BEFORE UPDATE ON public.applications
    FOR EACH ROW
    EXECUTE FUNCTION public.update_updated_at();

-- Webhooks table
DROP TRIGGER IF EXISTS trigger_webhooks_updated_at ON public.webhooks;
CREATE TRIGGER trigger_webhooks_updated_at
    BEFORE UPDATE ON public.webhooks
    FOR EACH ROW
    EXECUTE FUNCTION public.update_updated_at();
