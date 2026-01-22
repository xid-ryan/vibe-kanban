-- User ID Indexes for Multi-User Kubernetes Deployment
-- Creates indexes for efficient user_id filtering across all tables
--
-- Rollback procedure:
-- DROP INDEX IF EXISTS idx_projects_user_id;
-- DROP INDEX IF EXISTS idx_repos_user_id;
-- DROP INDEX IF EXISTS idx_tasks_user_id;
-- DROP INDEX IF EXISTS idx_workspaces_user_id;
-- DROP INDEX IF EXISTS idx_sessions_user_id;
-- DROP INDEX IF EXISTS idx_execution_processes_user_id;
-- DROP INDEX IF EXISTS idx_tasks_user_project;
-- DROP INDEX IF EXISTS idx_workspaces_user_task;
-- DROP INDEX IF EXISTS idx_sessions_user_workspace;
-- DROP INDEX IF EXISTS idx_execution_processes_user_session;
-- DROP INDEX IF EXISTS idx_repos_user_path;

-- ============================================================================
-- SIMPLE USER_ID INDEXES
-- These provide efficient filtering by user_id for isolation queries
-- ============================================================================

CREATE INDEX IF NOT EXISTS idx_projects_user_id ON projects(user_id);
CREATE INDEX IF NOT EXISTS idx_repos_user_id ON repos(user_id);
CREATE INDEX IF NOT EXISTS idx_tasks_user_id ON tasks(user_id);
CREATE INDEX IF NOT EXISTS idx_workspaces_user_id ON workspaces(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_execution_processes_user_id ON execution_processes(user_id);

-- ============================================================================
-- COMPOSITE INDEXES FOR COMMON QUERY PATTERNS
-- These optimize joins and filtered queries that are expected to be frequent
-- ============================================================================

-- Tasks: commonly queried by user + project
CREATE INDEX IF NOT EXISTS idx_tasks_user_project ON tasks(user_id, project_id);

-- Tasks: status filtering within a user's scope
CREATE INDEX IF NOT EXISTS idx_tasks_user_status ON tasks(user_id, status);

-- Workspaces: commonly queried by user + task
CREATE INDEX IF NOT EXISTS idx_workspaces_user_task ON workspaces(user_id, task_id);

-- Workspaces: archived/pinned filtering within user scope
CREATE INDEX IF NOT EXISTS idx_workspaces_user_archived ON workspaces(user_id, archived);
CREATE INDEX IF NOT EXISTS idx_workspaces_user_pinned ON workspaces(user_id, pinned)
    WHERE pinned = TRUE;

-- Sessions: commonly queried by user + workspace
CREATE INDEX IF NOT EXISTS idx_sessions_user_workspace ON sessions(user_id, workspace_id);

-- Execution processes: commonly queried by user + session
CREATE INDEX IF NOT EXISTS idx_execution_processes_user_session ON execution_processes(user_id, session_id);

-- Execution processes: status filtering within user scope
CREATE INDEX IF NOT EXISTS idx_execution_processes_user_status ON execution_processes(user_id, status);

-- Repos: unique path per user (supports the UNIQUE constraint)
CREATE INDEX IF NOT EXISTS idx_repos_user_path ON repos(user_id, path);

-- ============================================================================
-- PARTIAL INDEXES FOR SPECIFIC QUERY PATTERNS
-- These optimize queries that filter on specific status values
-- ============================================================================

-- Active execution processes per user (running status)
CREATE INDEX IF NOT EXISTS idx_execution_processes_user_running
    ON execution_processes(user_id, session_id)
    WHERE status = 'running';

-- Active sessions per user (recent sessions)
CREATE INDEX IF NOT EXISTS idx_sessions_user_recent
    ON sessions(user_id, created_at DESC);

-- ============================================================================
-- ANALYZE STATISTICS
-- Update table statistics after creating indexes
-- ============================================================================

ANALYZE projects;
ANALYZE repos;
ANALYZE tasks;
ANALYZE workspaces;
ANALYZE sessions;
ANALYZE execution_processes;
