-- PostgreSQL Migration: Ensure NOT NULL constraints on user_id columns
-- This migration ensures all user_id columns have NOT NULL constraints for multi-tenant isolation.
--
-- Note: In the initial schema (20260122000001), user_id columns were created as NOT NULL.
-- This migration exists to:
-- 1. Document the NOT NULL requirement explicitly
-- 2. Handle cases where migrations may have been applied out of order
-- 3. Support environments that may have had nullable user_id columns
--
-- Affected tables:
-- - projects
-- - tasks
-- - workspaces
-- - sessions
-- - execution_processes
-- - repos
--
-- Foreign Key Considerations:
-- - We intentionally do NOT add foreign key constraints on user_id to an external users table
-- - User identity is managed by an external Identity Provider (IdP) via JWT
-- - The user_id in these tables is the UUID from the JWT 'sub' claim
-- - This allows flexibility in user management without tight coupling to a local users table
--
-- Rollback procedure:
-- ALTER TABLE projects ALTER COLUMN user_id DROP NOT NULL;
-- ALTER TABLE tasks ALTER COLUMN user_id DROP NOT NULL;
-- ALTER TABLE workspaces ALTER COLUMN user_id DROP NOT NULL;
-- ALTER TABLE sessions ALTER COLUMN user_id DROP NOT NULL;
-- ALTER TABLE execution_processes ALTER COLUMN user_id DROP NOT NULL;
-- ALTER TABLE repos ALTER COLUMN user_id DROP NOT NULL;
--
-- WARNING: Rolling back NOT NULL constraints will break user isolation!

-- ============================================================================
-- IDEMPOTENT NOT NULL CONSTRAINT ADDITIONS
-- ============================================================================
-- These use DO blocks to check if the constraint already exists before adding
-- This makes the migration safe to run multiple times

-- Projects table
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'projects'
        AND column_name = 'user_id'
        AND is_nullable = 'YES'
    ) THEN
        -- First ensure no NULL values exist
        IF EXISTS (SELECT 1 FROM projects WHERE user_id IS NULL LIMIT 1) THEN
            RAISE EXCEPTION 'Cannot add NOT NULL constraint: projects table contains NULL user_id values. Please migrate or delete these rows first.';
        END IF;
        ALTER TABLE projects ALTER COLUMN user_id SET NOT NULL;
        RAISE NOTICE 'Added NOT NULL constraint to projects.user_id';
    ELSE
        RAISE NOTICE 'projects.user_id already has NOT NULL constraint';
    END IF;
END $$;

-- Tasks table
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'tasks'
        AND column_name = 'user_id'
        AND is_nullable = 'YES'
    ) THEN
        IF EXISTS (SELECT 1 FROM tasks WHERE user_id IS NULL LIMIT 1) THEN
            RAISE EXCEPTION 'Cannot add NOT NULL constraint: tasks table contains NULL user_id values. Please migrate or delete these rows first.';
        END IF;
        ALTER TABLE tasks ALTER COLUMN user_id SET NOT NULL;
        RAISE NOTICE 'Added NOT NULL constraint to tasks.user_id';
    ELSE
        RAISE NOTICE 'tasks.user_id already has NOT NULL constraint';
    END IF;
END $$;

-- Workspaces table
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'workspaces'
        AND column_name = 'user_id'
        AND is_nullable = 'YES'
    ) THEN
        IF EXISTS (SELECT 1 FROM workspaces WHERE user_id IS NULL LIMIT 1) THEN
            RAISE EXCEPTION 'Cannot add NOT NULL constraint: workspaces table contains NULL user_id values. Please migrate or delete these rows first.';
        END IF;
        ALTER TABLE workspaces ALTER COLUMN user_id SET NOT NULL;
        RAISE NOTICE 'Added NOT NULL constraint to workspaces.user_id';
    ELSE
        RAISE NOTICE 'workspaces.user_id already has NOT NULL constraint';
    END IF;
END $$;

-- Sessions table
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'sessions'
        AND column_name = 'user_id'
        AND is_nullable = 'YES'
    ) THEN
        IF EXISTS (SELECT 1 FROM sessions WHERE user_id IS NULL LIMIT 1) THEN
            RAISE EXCEPTION 'Cannot add NOT NULL constraint: sessions table contains NULL user_id values. Please migrate or delete these rows first.';
        END IF;
        ALTER TABLE sessions ALTER COLUMN user_id SET NOT NULL;
        RAISE NOTICE 'Added NOT NULL constraint to sessions.user_id';
    ELSE
        RAISE NOTICE 'sessions.user_id already has NOT NULL constraint';
    END IF;
END $$;

-- Execution processes table
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'execution_processes'
        AND column_name = 'user_id'
        AND is_nullable = 'YES'
    ) THEN
        IF EXISTS (SELECT 1 FROM execution_processes WHERE user_id IS NULL LIMIT 1) THEN
            RAISE EXCEPTION 'Cannot add NOT NULL constraint: execution_processes table contains NULL user_id values. Please migrate or delete these rows first.';
        END IF;
        ALTER TABLE execution_processes ALTER COLUMN user_id SET NOT NULL;
        RAISE NOTICE 'Added NOT NULL constraint to execution_processes.user_id';
    ELSE
        RAISE NOTICE 'execution_processes.user_id already has NOT NULL constraint';
    END IF;
END $$;

-- Repos table
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'repos'
        AND column_name = 'user_id'
        AND is_nullable = 'YES'
    ) THEN
        IF EXISTS (SELECT 1 FROM repos WHERE user_id IS NULL LIMIT 1) THEN
            RAISE EXCEPTION 'Cannot add NOT NULL constraint: repos table contains NULL user_id values. Please migrate or delete these rows first.';
        END IF;
        ALTER TABLE repos ALTER COLUMN user_id SET NOT NULL;
        RAISE NOTICE 'Added NOT NULL constraint to repos.user_id';
    ELSE
        RAISE NOTICE 'repos.user_id already has NOT NULL constraint';
    END IF;
END $$;

-- ============================================================================
-- VERIFICATION
-- ============================================================================
-- Verify all constraints are in place
DO $$
DECLARE
    nullable_count INTEGER;
BEGIN
    SELECT COUNT(*) INTO nullable_count
    FROM information_schema.columns
    WHERE table_name IN ('projects', 'tasks', 'workspaces', 'sessions', 'execution_processes', 'repos')
    AND column_name = 'user_id'
    AND is_nullable = 'YES';

    IF nullable_count > 0 THEN
        RAISE EXCEPTION 'Migration verification failed: % tables still have nullable user_id columns', nullable_count;
    END IF;

    RAISE NOTICE 'Verification passed: All user_id columns have NOT NULL constraint';
END $$;
