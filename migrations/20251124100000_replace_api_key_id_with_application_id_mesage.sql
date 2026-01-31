-- ============================================================================
-- Migration: Replace api_key_id with application_id in messages table
-- Description: API keys can be rotated, so we need to track by application_id
--              instead for stable message attribution and billing.
-- ============================================================================

BEGIN;

-- Step 1: Add application_id column to messages
ALTER TABLE public.messages
ADD COLUMN IF NOT EXISTS application_id uuid;

-- Step 2: Populate application_id from existing api_key_id via api_keys table
-- This migrates existing data by looking up the application_id for each api_key
UPDATE public.messages m
SET application_id = ak.application_id
FROM public.api_keys ak
WHERE m.api_key_id = ak.id
  AND m.application_id IS NULL;

-- Step 3: Add foreign key constraint to applications
ALTER TABLE public.messages
ADD CONSTRAINT messages_application_id_fkey
FOREIGN KEY (application_id)
REFERENCES public.applications (id)
ON UPDATE NO ACTION
ON DELETE CASCADE;

-- Step 4: Make application_id NOT NULL after data migration
-- (Only do this if all rows have been migrated successfully)
ALTER TABLE public.messages
ALTER COLUMN application_id SET NOT NULL;

-- Step 5: Drop the old api_key_id foreign key constraint
ALTER TABLE public.messages
DROP CONSTRAINT IF EXISTS messages_api_key_id_fkey;

-- Step 6: Drop the old api_key_id column
ALTER TABLE public.messages
DROP COLUMN IF EXISTS api_key_id;

-- Step 7: Create index for application_id lookups
CREATE INDEX IF NOT EXISTS idx_messages_application_id
    ON public.messages USING btree (application_id ASC NULLS LAST)
    TABLESPACE pg_default;

-- Step 8: Drop the old api_key index
DROP INDEX IF EXISTS idx_messages_api_key;


-- Optionally create a new trigger to update application's updated_at
CREATE OR REPLACE FUNCTION public.update_application_last_message()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE public.applications
    SET updated_at = NOW()
    WHERE id = NEW.application_id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_update_application_last_message
    AFTER INSERT ON public.messages
    FOR EACH ROW
    EXECUTE FUNCTION public.update_application_last_message();

COMMIT;

-- ============================================================================
-- Verification Queries
-- ============================================================================
-- Verify application_id column exists and api_key_id is gone
-- SELECT column_name, data_type, is_nullable
-- FROM information_schema.columns
-- WHERE table_name = 'messages' AND column_name IN ('application_id', 'api_key_id');

-- Verify foreign key constraint
-- SELECT constraint_name, table_name
-- FROM information_schema.table_constraints
-- WHERE table_name = 'messages' AND constraint_type = 'FOREIGN KEY';

-- Verify all messages have application_id
-- SELECT COUNT(*) as total, COUNT(application_id) as with_app_id FROM messages;
