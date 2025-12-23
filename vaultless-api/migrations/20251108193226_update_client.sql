-- 20251108193226_update_client.sql
-- ============================================================================
-- Migration: Update clients table for application support
-- ============================================================================

BEGIN;

-- Step 1: Make application_id NOT NULL (if you want strict enforcement)
-- IMPORTANT: Only run this if all existing clients have application_id set
-- If you have existing clients without application_id, handle them first
ALTER TABLE public.clients 
ALTER COLUMN application_id SET NOT NULL;

-- Step 2: Fix foreign key constraint to CASCADE delete
ALTER TABLE public.clients 
DROP CONSTRAINT IF EXISTS clients_application_id_fkey;

ALTER TABLE public.clients
ADD CONSTRAINT clients_application_id_fkey 
FOREIGN KEY (application_id)
REFERENCES public.applications (id)
ON UPDATE NO ACTION
ON DELETE CASCADE;

-- Step 3: Add composite indexes for common queries
CREATE INDEX IF NOT EXISTS idx_clients_app_active
    ON public.clients USING btree 
    (application_id ASC NULLS LAST, is_active ASC NULLS LAST)
    TABLESPACE pg_default
    WHERE is_active = true;


COMMIT;

-- ============================================================================
-- Verification queries
-- ============================================================================

-- Verify foreign key
SELECT 
    conname AS constraint_name,
    conrelid::regclass AS table_name,
    confrelid::regclass AS referenced_table,
    confdeltype AS on_delete_action
FROM pg_constraint
WHERE conname = 'clients_application_id_fkey';
-- Expected: on_delete_action = 'c' (CASCADE)

-- Verify indexes
SELECT 
    schemaname,
    tablename,
    indexname,
    indexdef
FROM pg_indexes
WHERE tablename = 'clients'
  AND indexname LIKE 'idx_clients_app%'
ORDER BY indexname;

-- Verify NOT NULL constraint
SELECT 
    column_name,
    is_nullable
FROM information_schema.columns
WHERE table_name = 'clients'
  AND column_name = 'application_id';
-- Expected: is_nullable = 'NO'
