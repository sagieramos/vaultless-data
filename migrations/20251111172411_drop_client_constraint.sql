-- Migration Script: Reverse Key Ownership (One Application to Many API Keys)

-- 1. CLEANUP: REMOVE OBSOLETE KEY COLUMNS AND CONSTRAINTS FROM 'applications'
----------------------------------------------------------------------------------------------------

-- If a column named 'secret_key_id' (or the old 'api_key_id') exists, drop it.
-- This ensures 'applications' is clean of direct key references.
ALTER TABLE public.applications
    DROP COLUMN IF EXISTS secret_key_id;

-- Ensure all related foreign key and unique constraints are dropped
ALTER TABLE public.applications
    DROP CONSTRAINT IF EXISTS applications_secret_key_id_fkey,
    DROP CONSTRAINT IF EXISTS applications_secret_key_id_key;

-- Drop old key-related indexes on the applications table
DROP INDEX IF EXISTS public.idx_applications_secret_key_id;
DROP INDEX IF EXISTS public.idx_applications_api_key_id;


-- 2. REFOCUS: ADD application_id FOREIGN KEY TO 'api_keys'
----------------------------------------------------------------------------------------------------

-- Drop constraint related to 'clients' that is no longer needed
ALTER TABLE public.clients
DROP CONSTRAINT IF EXISTS clients_application_consistency_check;

DROP INDEX IF EXISTS public.idx_clients_dev_api;

-- Add the application_id foreign key column to api_keys
ALTER TABLE public.api_keys
ADD COLUMN IF NOT EXISTS application_id uuid;

-- Add the foreign key constraint (REFERENCES public.applications)
-- The ON DELETE CASCADE ensures keys are cleaned up if the application is deleted.
ALTER TABLE public.api_keys
ADD CONSTRAINT api_keys_application_id_fkey FOREIGN KEY (application_id)
    REFERENCES public.applications (id) MATCH SIMPLE
    ON UPDATE NO ACTION
    ON DELETE CASCADE;

-- Add the index for fast lookups by application
CREATE INDEX IF NOT EXISTS idx_api_keys_application_id
    ON public.api_keys USING btree
    (application_id ASC NULLS LAST)
    TABLESPACE pg_default;



-- Create Key Type ENUM
CREATE TYPE key_type AS ENUM ('secret', 'publishable');

ALTER TABLE public.api_keys
ADD COLUMN IF NOT EXISTS key_type key_type NOT NULL DEFAULT 'secret';

-- 4. REFOCUS: HASHED VS. PLAINTEXT KEYS
----------------------------------------------------------------------------------------------------

-- Add publishable key plaintext column (max 64 chars is standard for prefix + key)
ALTER TABLE public.api_keys
    ADD COLUMN IF NOT EXISTS publishable_key_plaintext character varying(128) COLLATE pg_catalog."default";

-- Drop and re-add unique constraint on key_hash to allow NULLs for publishable keys
ALTER TABLE public.api_keys
    DROP CONSTRAINT IF EXISTS api_keys_key_hash_key;

ALTER TABLE public.api_keys
    ALTER COLUMN key_hash DROP NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS api_keys_key_hash_key ON public.api_keys (key_hash) 
    WHERE key_hash IS NOT NULL;

-- Index on plaintext key, only for publishable keys (which are not hashed)
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_keys_publishable_key_plaintext
    ON public.api_keys USING btree
    (publishable_key_plaintext COLLATE pg_catalog."default" ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE key_type = 'publishable';
    
-- Add the final key data check constraint to enforce key_type validity
ALTER TABLE public.api_keys
ADD CONSTRAINT required_key_data_check CHECK (
    key_prefix IS NOT NULL AND -- All keys must have a prefix
    (
        -- Case 1: Secret Key must be hashed, plaintext must be NULL
        key_type = 'secret'::key_type AND 
        key_hash IS NOT NULL AND 
        publishable_key_plaintext IS NULL
    ) 
    OR 
    (
        -- Case 2: Publishable Key must be plaintext, hash must be NULL
        key_type = 'publishable'::key_type AND 
        publishable_key_plaintext IS NOT NULL AND 
        key_hash IS NULL
    )
);
