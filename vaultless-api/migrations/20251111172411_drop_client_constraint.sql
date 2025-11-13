-- Add migration script here
ALTER TABLE public.clients
DROP CONSTRAINT IF EXISTS clients_application_consistency_check;

DROP INDEX IF EXISTS public.idx_clients_dev_api;

-- Add the application_id foreign key column to api_keys
ALTER TABLE public.api_keys
ADD COLUMN application_id uuid;

-- Add the foreign key constraint (REFERENCES public.applications)
-- We defer adding the NOT NULL constraint until after data migration
ALTER TABLE public.api_keys
ADD CONSTRAINT api_keys_application_id_fkey FOREIGN KEY (application_id)
    REFERENCES public.applications (id) MATCH SIMPLE
    ON UPDATE NO ACTION
    ON DELETE CASCADE;

-- Add the index
CREATE INDEX IF NOT EXISTS idx_api_keys_application_id
    ON public.api_keys USING btree
    (application_id ASC NULLS LAST)
    TABLESPACE pg_default;

-- Make usage-related fields nullable and drop the old NOT NULL constraints
ALTER TABLE public.api_keys
ALTER COLUMN tier DROP NOT NULL,
ALTER COLUMN monthly_message_quota DROP NOT NULL,
ALTER COLUMN message_retention_seconds DROP NOT NULL,
ALTER COLUMN rate_limit_per_minute DROP NOT NULL;

-- Remove the old CHECK constraints that relied on NOT NULL, or update them 
-- to be conditional based on key_type. The conditional approach is best:
ALTER TABLE public.api_keys
DROP CONSTRAINT valid_quota,
DROP CONSTRAINT valid_retention;

CREATE TYPE key_type AS ENUM ('secret', 'publishable');

ALTER TABLE public.api_keys 
ADD COLUMN IF NOT EXISTS key_type key_type NOT NULL DEFAULT 'secret';

ALTER TABLE public.api_keys
ADD CONSTRAINT valid_quota CHECK (CASE WHEN key_type = 'secret' THEN monthly_message_quota > 0 ELSE true END);

ALTER TABLE public.api_keys
ADD CONSTRAINT valid_retention CHECK (CASE WHEN key_type = 'secret' THEN message_retention_seconds > 0 ELSE true END);

-- Rename the column for clarity
ALTER TABLE public.applications
RENAME COLUMN api_key_id TO secret_key_id;

-- Drop and re-add the Foreign Key constraint with the new name
ALTER TABLE public.applications
DROP CONSTRAINT applications_api_key_id_fkey,
ADD CONSTRAINT applications_secret_key_id_fkey FOREIGN KEY (secret_key_id)
    REFERENCES public.api_keys (id) MATCH SIMPLE
    ON UPDATE NO ACTION
    ON DELETE CASCADE;

-- Add the unique constraint to enforce the 1:1 relationship
ALTER TABLE public.applications
ADD CONSTRAINT applications_secret_key_id_key UNIQUE (secret_key_id);

-- Update the index name
DROP INDEX public.idx_applications_api_key_id;
CREATE INDEX IF NOT EXISTS idx_applications_secret_key_id
    ON public.applications USING btree
    (secret_key_id ASC NULLS LAST)
    TABLESPACE pg_default;

ALTER TABLE public.api_keys
    ADD COLUMN publishable_key_plaintext character varying(64) COLLATE pg_catalog."default";

-- A. Drop the UNIQUE constraint (if it was applied to the NOT NULL column)
ALTER TABLE public.api_keys
    DROP CONSTRAINT api_keys_key_hash_key;

-- B. Drop the NOT NULL constraint on key_hash
ALTER TABLE public.api_keys
    ALTER COLUMN key_hash DROP NOT NULL;

-- C. Re-add the UNIQUE constraint, allowing multiple NULLs (standard for PostgreSQL)
CREATE UNIQUE INDEX IF NOT EXISTS api_keys_key_hash_key ON public.api_keys (key_hash) 
    WHERE key_hash IS NOT NULL;


-- Index on plaintext key, only for publishable keys
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_publishable_key_plaintext
    ON public.api_keys USING btree
    (publishable_key_plaintext COLLATE pg_catalog."default" ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE key_type = 'publishable';
    
ALTER TABLE public.api_keys
ADD CONSTRAINT required_key_data_check CHECK (
    key_prefix IS NOT NULL AND -- Explicitly enforce prefix presence
    (
        -- Case 1: Secret Key
        key_type = 'secret'::key_type AND 
        key_hash IS NOT NULL AND 
        publishable_key_plaintext IS NULL
    ) 
    OR 
    (
        -- Case 2: Publishable Key
        key_type = 'publishable'::key_type AND 
        publishable_key_plaintext IS NOT NULL AND 
        key_hash IS NULL
    )
);