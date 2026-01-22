-- PTY Sessions Tracking Table for Multi-User Kubernetes Deployment
-- Tracks active terminal sessions for user isolation and cleanup
--
-- Rollback procedure:
-- DROP TABLE IF EXISTS pty_sessions;

-- ============================================================================
-- PTY_SESSIONS
-- ============================================================================
CREATE TABLE IF NOT EXISTS pty_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_pty_sessions_user_id ON pty_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_pty_sessions_workspace_id ON pty_sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_pty_sessions_activity ON pty_sessions(last_activity_at);
CREATE INDEX IF NOT EXISTS idx_pty_sessions_user_activity ON pty_sessions(user_id, last_activity_at);

COMMENT ON TABLE pty_sessions IS 'Active PTY (terminal) sessions for users';
COMMENT ON COLUMN pty_sessions.user_id IS 'Owner user ID for multi-tenant isolation';
COMMENT ON COLUMN pty_sessions.workspace_id IS 'Optional workspace this session is associated with';
COMMENT ON COLUMN pty_sessions.last_activity_at IS 'Timestamp of last activity for idle cleanup';
