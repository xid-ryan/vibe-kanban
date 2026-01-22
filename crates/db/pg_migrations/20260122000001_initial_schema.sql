-- PostgreSQL Initial Schema for Multi-User Kubernetes Deployment
-- This migration creates all tables with user_id columns for user isolation
--
-- Rollback procedure:
-- DROP TABLE IF EXISTS task_images CASCADE;
-- DROP TABLE IF EXISTS images CASCADE;
-- DROP TABLE IF EXISTS execution_process_repo_states CASCADE;
-- DROP TABLE IF EXISTS execution_process_logs CASCADE;
-- DROP TABLE IF EXISTS coding_agent_turns CASCADE;
-- DROP TABLE IF EXISTS execution_processes CASCADE;
-- DROP TABLE IF EXISTS sessions CASCADE;
-- DROP TABLE IF EXISTS merges CASCADE;
-- DROP TABLE IF EXISTS workspace_repos CASCADE;
-- DROP TABLE IF EXISTS workspaces CASCADE;
-- DROP TABLE IF EXISTS tasks CASCADE;
-- DROP TABLE IF EXISTS project_repos CASCADE;
-- DROP TABLE IF EXISTS repos CASCADE;
-- DROP TABLE IF EXISTS projects CASCADE;

-- Enable UUID extension if not already enabled
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================================================
-- PROJECTS
-- ============================================================================
CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    name TEXT NOT NULL,
    remote_project_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_remote_project_id
    ON projects(remote_project_id)
    WHERE remote_project_id IS NOT NULL;

COMMENT ON TABLE projects IS 'User projects containing tasks and linked repositories';
COMMENT ON COLUMN projects.user_id IS 'Owner user ID for multi-tenant isolation';

-- ============================================================================
-- REPOS
-- ============================================================================
CREATE TABLE IF NOT EXISTS repos (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    setup_script TEXT,
    cleanup_script TEXT,
    copy_files TEXT,
    parallel_setup_script BOOLEAN NOT NULL DEFAULT FALSE,
    dev_server_script TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, path)
);

COMMENT ON TABLE repos IS 'Git repositories registered by users';
COMMENT ON COLUMN repos.user_id IS 'Owner user ID for multi-tenant isolation';
COMMENT ON COLUMN repos.path IS 'Absolute path to the repository';

-- ============================================================================
-- PROJECT_REPOS (Junction table - no user_id, inherits from project/repo)
-- ============================================================================
CREATE TABLE IF NOT EXISTS project_repos (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    repo_id UUID NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    UNIQUE (project_id, repo_id)
);

CREATE INDEX IF NOT EXISTS idx_project_repos_project_id ON project_repos(project_id);
CREATE INDEX IF NOT EXISTS idx_project_repos_repo_id ON project_repos(repo_id);

COMMENT ON TABLE project_repos IS 'Junction table linking projects to repositories';

-- ============================================================================
-- TASKS
-- ============================================================================
CREATE TABLE IF NOT EXISTS tasks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_workspace_id UUID,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'todo'
        CHECK (status IN ('todo', 'inprogress', 'done', 'cancelled', 'inreview')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_tasks_parent_workspace_id ON tasks(parent_workspace_id);

COMMENT ON TABLE tasks IS 'User tasks within projects';
COMMENT ON COLUMN tasks.user_id IS 'Owner user ID for multi-tenant isolation';

-- ============================================================================
-- WORKSPACES (formerly task_attempts)
-- ============================================================================
CREATE TABLE IF NOT EXISTS workspaces (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    branch TEXT NOT NULL,
    container_ref TEXT,
    agent_working_dir TEXT,
    archived BOOLEAN NOT NULL DEFAULT FALSE,
    pinned BOOLEAN NOT NULL DEFAULT FALSE,
    name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_workspaces_task_id ON workspaces(task_id);

COMMENT ON TABLE workspaces IS 'Workspace environments for task execution';
COMMENT ON COLUMN workspaces.user_id IS 'Owner user ID for multi-tenant isolation';
COMMENT ON COLUMN workspaces.branch IS 'Git branch for this workspace';
COMMENT ON COLUMN workspaces.container_ref IS 'Path to workspace directory';

-- Add foreign key for parent_workspace_id after workspaces table exists
ALTER TABLE tasks
    ADD CONSTRAINT fk_tasks_parent_workspace
    FOREIGN KEY (parent_workspace_id) REFERENCES workspaces(id) ON DELETE SET NULL;

-- ============================================================================
-- WORKSPACE_REPOS
-- ============================================================================
CREATE TABLE IF NOT EXISTS workspace_repos (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repo_id UUID NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    target_branch TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (workspace_id, repo_id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_repos_workspace_id ON workspace_repos(workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_repos_repo_id ON workspace_repos(repo_id);
CREATE INDEX IF NOT EXISTS idx_workspace_repos_lookup ON workspace_repos(workspace_id, repo_id);

COMMENT ON TABLE workspace_repos IS 'Junction table linking workspaces to repositories with branch info';

-- ============================================================================
-- SESSIONS
-- ============================================================================
CREATE TABLE IF NOT EXISTS sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    executor TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sessions_workspace_id ON sessions(workspace_id);

COMMENT ON TABLE sessions IS 'Execution sessions within workspaces';
COMMENT ON COLUMN sessions.user_id IS 'Owner user ID for multi-tenant isolation';
COMMENT ON COLUMN sessions.executor IS 'Executor type (e.g., CLAUDE_CODE, CODEX)';

-- ============================================================================
-- MERGES
-- ============================================================================
CREATE TABLE IF NOT EXISTS merges (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    repo_id UUID NOT NULL REFERENCES repos(id),
    merge_type TEXT NOT NULL CHECK (merge_type IN ('direct', 'pr')),
    target_branch_name TEXT NOT NULL,

    -- Direct merge fields (NULL for PR merges)
    merge_commit TEXT,

    -- PR merge fields (NULL for direct merges)
    pr_number INTEGER,
    pr_url TEXT,
    pr_status TEXT CHECK (pr_status IN ('open', 'merged', 'closed')),
    pr_merged_at TIMESTAMPTZ,
    pr_merge_commit_sha TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Data integrity constraints
    CHECK (
        (merge_type = 'direct' AND merge_commit IS NOT NULL
         AND pr_number IS NULL AND pr_url IS NULL)
        OR
        (merge_type = 'pr' AND pr_number IS NOT NULL AND pr_url IS NOT NULL
         AND pr_status IS NOT NULL AND merge_commit IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_merges_workspace_id ON merges(workspace_id);
CREATE INDEX IF NOT EXISTS idx_merges_repo_id ON merges(repo_id);
CREATE INDEX IF NOT EXISTS idx_merges_open_pr ON merges(workspace_id, pr_status)
    WHERE merge_type = 'pr' AND pr_status = 'open';
CREATE INDEX IF NOT EXISTS idx_merges_type_status ON merges(merge_type, pr_status);

COMMENT ON TABLE merges IS 'Merge operations (direct or PR) for workspaces';

-- ============================================================================
-- EXECUTION_PROCESSES
-- ============================================================================
CREATE TABLE IF NOT EXISTS execution_processes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_reason TEXT NOT NULL DEFAULT 'setupscript'
        CHECK (run_reason IN ('setupscript', 'codingagent', 'devserver', 'cleanupscript')),
    executor_action JSONB NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'running'
        CHECK (status IN ('running', 'completed', 'failed', 'killed')),
    exit_code INTEGER,
    dropped BOOLEAN NOT NULL DEFAULT FALSE,
    masked_by_restore BOOLEAN NOT NULL DEFAULT FALSE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_execution_processes_session_id ON execution_processes(session_id);
CREATE INDEX IF NOT EXISTS idx_execution_processes_status ON execution_processes(status);
CREATE INDEX IF NOT EXISTS idx_execution_processes_run_reason ON execution_processes(run_reason);
CREATE INDEX IF NOT EXISTS idx_execution_processes_session_status_run_reason
    ON execution_processes(session_id, status, run_reason);
CREATE INDEX IF NOT EXISTS idx_execution_processes_session_run_reason_created
    ON execution_processes(session_id, run_reason, created_at DESC);

COMMENT ON TABLE execution_processes IS 'Process executions within sessions (setup scripts, coding agents, etc.)';
COMMENT ON COLUMN execution_processes.user_id IS 'Owner user ID for multi-tenant isolation';

-- ============================================================================
-- EXECUTION_PROCESS_LOGS
-- ============================================================================
CREATE TABLE IF NOT EXISTS execution_process_logs (
    execution_id UUID PRIMARY KEY REFERENCES execution_processes(id) ON DELETE CASCADE,
    logs TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_execution_process_logs_inserted_at ON execution_process_logs(inserted_at);

COMMENT ON TABLE execution_process_logs IS 'JSONL logs for execution processes';

-- ============================================================================
-- EXECUTION_PROCESS_REPO_STATES
-- ============================================================================
CREATE TABLE IF NOT EXISTS execution_process_repo_states (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    execution_process_id UUID NOT NULL REFERENCES execution_processes(id) ON DELETE CASCADE,
    repo_id UUID NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    before_head_commit TEXT,
    after_head_commit TEXT,
    merge_commit TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (execution_process_id, repo_id)
);

CREATE INDEX IF NOT EXISTS idx_eprs_process_id ON execution_process_repo_states(execution_process_id);
CREATE INDEX IF NOT EXISTS idx_eprs_repo_id ON execution_process_repo_states(repo_id);

COMMENT ON TABLE execution_process_repo_states IS 'Git state before/after execution processes per repository';

-- ============================================================================
-- CODING_AGENT_TURNS
-- ============================================================================
CREATE TABLE IF NOT EXISTS coding_agent_turns (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    execution_process_id UUID NOT NULL REFERENCES execution_processes(id) ON DELETE CASCADE,
    agent_session_id TEXT,
    prompt TEXT,
    summary TEXT,
    seen BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_coding_agent_turns_execution_process_id ON coding_agent_turns(execution_process_id);
CREATE INDEX IF NOT EXISTS idx_coding_agent_turns_agent_session_id ON coding_agent_turns(agent_session_id);

COMMENT ON TABLE coding_agent_turns IS 'Individual turns/interactions with coding agents';

-- ============================================================================
-- IMAGES
-- ============================================================================
CREATE TABLE IF NOT EXISTS images (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_path TEXT NOT NULL,
    original_name TEXT NOT NULL,
    mime_type TEXT,
    size_bytes INTEGER,
    hash TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_images_hash ON images(hash);

COMMENT ON TABLE images IS 'Image file metadata for deduplication';
COMMENT ON COLUMN images.hash IS 'SHA256 hash for deduplication';

-- ============================================================================
-- TASK_IMAGES
-- ============================================================================
CREATE TABLE IF NOT EXISTS task_images (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    image_id UUID NOT NULL REFERENCES images(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (task_id, image_id)
);

CREATE INDEX IF NOT EXISTS idx_task_images_task_id ON task_images(task_id);
CREATE INDEX IF NOT EXISTS idx_task_images_image_id ON task_images(image_id);

COMMENT ON TABLE task_images IS 'Junction table linking tasks to images';
