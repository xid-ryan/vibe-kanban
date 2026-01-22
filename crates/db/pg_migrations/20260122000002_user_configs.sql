-- User Configurations Table for Multi-User Kubernetes Deployment
-- Stores user-specific application settings and encrypted OAuth credentials
--
-- Rollback procedure:
-- DROP TRIGGER IF EXISTS update_user_configs_updated_at ON user_configs;
-- DROP FUNCTION IF EXISTS update_updated_at_column();
-- DROP TABLE IF EXISTS user_configs;

-- ============================================================================
-- USER_CONFIGS
-- ============================================================================
CREATE TABLE IF NOT EXISTS user_configs (
    user_id UUID PRIMARY KEY,
    config_json JSONB NOT NULL DEFAULT '{}',
    oauth_credentials BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE user_configs IS 'User-specific application configuration';
COMMENT ON COLUMN user_configs.user_id IS 'User ID from JWT claims (primary key)';
COMMENT ON COLUMN user_configs.config_json IS 'JSON configuration object with user preferences';
COMMENT ON COLUMN user_configs.oauth_credentials IS 'AES-256-GCM encrypted OAuth credentials';

-- ============================================================================
-- AUTOMATIC updated_at TRIGGER
-- ============================================================================

-- Create the trigger function if it doesn't exist
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger for automatic updated_at timestamp
DROP TRIGGER IF EXISTS update_user_configs_updated_at ON user_configs;
CREATE TRIGGER update_user_configs_updated_at
    BEFORE UPDATE ON user_configs
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
