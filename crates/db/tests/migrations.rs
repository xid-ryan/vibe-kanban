//! PostgreSQL migration tests for multi-user Kubernetes deployment.
//!
//! These tests verify that:
//! - Migrations run successfully on a fresh database
//! - Migrations are idempotent (can be run multiple times safely)
//! - All expected tables are created with correct schema
//! - User isolation indexes are properly created
//! - Constraints and triggers are correctly applied
//!
//! # Requirements
//!
//! These tests require a running PostgreSQL instance. Set the `DATABASE_URL`
//! environment variable to a valid PostgreSQL connection string before running.
//!
//! # Running Tests
//!
//! ```bash
//! # Run all migration tests (requires PostgreSQL)
//! DATABASE_URL="postgres://user:pass@localhost/test_db" \
//!   cargo test -p db --test migrations -- --ignored --test-threads=1
//!
//! # Run only unit tests (no PostgreSQL required)
//! cargo test -p db --test migrations -- --test-threads=1
//! ```

use std::env;

/// Environment variable name for database URL.
const DATABASE_URL_ENV: &str = "DATABASE_URL";

// ============================================================================
// Unit Tests (No PostgreSQL Required)
// ============================================================================

/// MIG-UNIT-01: Verify migration file naming convention
#[test]
fn mig_unit_01_migration_naming_convention() {
    // Migration files should follow the pattern: YYYYMMDDHHMMSS_description.sql
    let migration_files = vec![
        "20260122000001_initial_schema.sql",
        "20260122000002_user_configs.sql",
        "20260122000003_pty_sessions.sql",
        "20260122000004_user_indexes.sql",
        "20260122000005_user_id_not_null.sql",
    ];

    for file in &migration_files {
        // Verify naming pattern: starts with timestamp
        assert!(
            file.starts_with("2026"),
            "Migration {} should start with year",
            file
        );

        // Verify extension
        assert!(
            file.ends_with(".sql"),
            "Migration {} should have .sql extension",
            file
        );

        // Verify underscore separator after timestamp
        let parts: Vec<&str> = file.splitn(2, '_').collect();
        assert_eq!(parts.len(), 2, "Migration {} should have timestamp_name format", file);

        // Verify timestamp is numeric
        let timestamp = parts[0];
        assert!(
            timestamp.chars().all(|c| c.is_ascii_digit()),
            "Timestamp {} should be numeric",
            timestamp
        );
    }
}

/// MIG-UNIT-02: Verify expected number of migrations
#[test]
fn mig_unit_02_expected_migration_count() {
    // We expect 5 migrations for the multi-user deployment
    let expected_count = 5;

    // Migration versions in order
    let versions = vec![
        "20260122000001", // initial_schema
        "20260122000002", // user_configs
        "20260122000003", // pty_sessions
        "20260122000004", // user_indexes
        "20260122000005", // user_id_not_null
    ];

    assert_eq!(
        versions.len(),
        expected_count,
        "Expected {} migrations",
        expected_count
    );

    // Verify versions are in ascending order
    for i in 1..versions.len() {
        assert!(
            versions[i] > versions[i - 1],
            "Migrations should be in ascending order: {} should be after {}",
            versions[i],
            versions[i - 1]
        );
    }
}

/// MIG-UNIT-03: Verify migration descriptions are meaningful
#[test]
fn mig_unit_03_migration_descriptions() {
    let descriptions = vec![
        ("initial_schema", "Creates all base tables"),
        ("user_configs", "Creates user configuration table"),
        ("pty_sessions", "Creates PTY sessions tracking table"),
        ("user_indexes", "Creates indexes for user_id filtering"),
        ("user_id_not_null", "Ensures NOT NULL on user_id columns"),
    ];

    for (name, purpose) in descriptions {
        assert!(
            !name.is_empty(),
            "Migration name should not be empty: {}",
            purpose
        );
        assert!(
            name.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "Migration name should be alphanumeric with underscores: {}",
            name
        );
    }
}

/// MIG-UNIT-04: Verify expected tables from migrations
#[test]
fn mig_unit_04_expected_tables() {
    let expected_tables = vec![
        // From initial_schema
        "projects",
        "repos",
        "project_repos",
        "tasks",
        "workspaces",
        "workspace_repos",
        "sessions",
        "merges",
        "execution_processes",
        "execution_process_logs",
        "execution_process_repo_states",
        "coding_agent_turns",
        "images",
        "task_images",
        // From user_configs
        "user_configs",
        // From pty_sessions
        "pty_sessions",
    ];

    // Verify no duplicates
    let mut unique_tables = expected_tables.clone();
    unique_tables.sort();
    unique_tables.dedup();
    assert_eq!(
        expected_tables.len(),
        unique_tables.len(),
        "Table names should be unique"
    );

    // Verify naming convention (snake_case)
    for table in &expected_tables {
        assert!(
            table.chars().all(|c| c.is_lowercase() || c == '_'),
            "Table {} should be snake_case",
            table
        );
    }
}

/// MIG-UNIT-05: Verify expected indexes from migrations
#[test]
fn mig_unit_05_expected_indexes() {
    let expected_indexes = vec![
        // Simple user_id indexes
        "idx_projects_user_id",
        "idx_repos_user_id",
        "idx_tasks_user_id",
        "idx_workspaces_user_id",
        "idx_sessions_user_id",
        "idx_execution_processes_user_id",
        // Composite indexes
        "idx_tasks_user_project",
        "idx_tasks_user_status",
        "idx_workspaces_user_task",
        "idx_workspaces_user_archived",
        "idx_workspaces_user_pinned",
        "idx_sessions_user_workspace",
        "idx_execution_processes_user_session",
        "idx_execution_processes_user_status",
        "idx_repos_user_path",
        // PTY session indexes
        "idx_pty_sessions_user_id",
        "idx_pty_sessions_workspace_id",
        "idx_pty_sessions_activity",
        "idx_pty_sessions_user_activity",
    ];

    for index in &expected_indexes {
        // Verify index naming convention
        assert!(
            index.starts_with("idx_"),
            "Index {} should start with idx_",
            index
        );
    }
}

/// MIG-UNIT-06: Verify tables requiring user_id columns
#[test]
fn mig_unit_06_tables_with_user_id() {
    // Tables that should have user_id column for multi-tenant isolation
    let tables_with_user_id = vec![
        "projects",
        "tasks",
        "workspaces",
        "sessions",
        "execution_processes",
        "repos",
        "pty_sessions",
    ];

    // Tables that should NOT have user_id (junction tables, etc.)
    let tables_without_user_id = vec![
        "project_repos",      // Junction table
        "workspace_repos",    // Junction table
        "task_images",        // Junction table
        "images",             // Deduplicated, no user scope
        "merges",             // Inherits from workspace
        "execution_process_logs",        // Inherits from execution_process
        "execution_process_repo_states", // Inherits from execution_process
        "coding_agent_turns", // Inherits from execution_process
    ];

    // Verify no overlap
    for table in &tables_with_user_id {
        assert!(
            !tables_without_user_id.contains(table),
            "Table {} should not be in both lists",
            table
        );
    }
}

/// MIG-UNIT-07: Verify user_configs table schema expectations
#[test]
fn mig_unit_07_user_configs_schema() {
    // Expected columns for user_configs
    let columns = vec![
        ("user_id", "UUID", true),        // PRIMARY KEY
        ("config_json", "JSONB", false),  // NOT NULL, DEFAULT '{}'
        ("oauth_credentials", "BYTEA", true), // Nullable, encrypted
        ("created_at", "TIMESTAMPTZ", false), // NOT NULL, DEFAULT NOW()
        ("updated_at", "TIMESTAMPTZ", false), // NOT NULL, DEFAULT NOW()
    ];

    for (name, data_type, nullable) in columns {
        assert!(
            !name.is_empty(),
            "Column name should not be empty for {}",
            data_type
        );
        // Verify column naming convention
        assert!(
            name.chars().all(|c| c.is_lowercase() || c == '_'),
            "Column {} should be snake_case",
            name
        );
        // Log expectation
        let null_str = if nullable { "NULL" } else { "NOT NULL" };
        println!("user_configs.{}: {} {}", name, data_type, null_str);
    }
}

/// MIG-UNIT-08: Verify pty_sessions table schema expectations
#[test]
fn mig_unit_08_pty_sessions_schema() {
    // Expected columns for pty_sessions
    let columns = vec![
        ("id", "UUID", false),              // PRIMARY KEY
        ("user_id", "UUID", false),         // NOT NULL
        ("workspace_id", "UUID", true),     // Nullable (ON DELETE SET NULL)
        ("created_at", "TIMESTAMPTZ", false),
        ("last_activity_at", "TIMESTAMPTZ", false), // For idle cleanup
    ];

    for (name, data_type, nullable) in columns {
        assert!(
            !name.is_empty(),
            "Column name should not be empty for {}",
            data_type
        );
        println!(
            "pty_sessions.{}: {} {}",
            name,
            data_type,
            if nullable { "NULL" } else { "NOT NULL" }
        );
    }
}

// ============================================================================
// Integration Tests (Require PostgreSQL)
// ============================================================================

/// Get DATABASE_URL or skip test
fn get_database_url() -> Option<String> {
    env::var(DATABASE_URL_ENV).ok()
}

/// MIG-01: Fresh database migration execution
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_01_fresh_migration_execution() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");

    // Create database service which runs migrations
    let result = db::DBServicePg::new_with_url(&database_url, 5).await;
    assert!(
        result.is_ok(),
        "Migrations should run successfully on fresh database: {:?}",
        result.err()
    );

    let service = result.unwrap();

    // Verify database is accessible
    let health = service.health_check().await;
    assert!(health.is_ok(), "Health check should pass after migrations");
}

/// MIG-02: Migration idempotency - run twice
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_02_migration_idempotency() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");

    // First run
    let service1 = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("First migration run should succeed");

    // Second run on same database
    let service2 = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Second migration run should succeed (idempotency)");

    // Both should pass health check
    assert!(service1.health_check().await.is_ok());
    assert!(service2.health_check().await.is_ok());
}

/// MIG-03: Projects table schema validation
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_03_projects_table_schema() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Query for projects table columns
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        r#"
        SELECT column_name, data_type, is_nullable
        FROM information_schema.columns
        WHERE table_name = 'projects'
        ORDER BY ordinal_position
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query columns");

    // Verify user_id column exists and is NOT NULL
    let user_id_col = columns.iter().find(|(name, _, _)| name == "user_id");
    assert!(user_id_col.is_some(), "projects should have user_id column");

    let (_, _, nullable) = user_id_col.unwrap();
    assert_eq!(nullable, "NO", "user_id should be NOT NULL");
}

/// MIG-11: User ID indexes exist
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_11_user_id_indexes_exist() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    let expected_indexes = vec![
        "idx_projects_user_id",
        "idx_repos_user_id",
        "idx_tasks_user_id",
        "idx_workspaces_user_id",
        "idx_sessions_user_id",
        "idx_execution_processes_user_id",
    ];

    // Query pg_indexes for our indexes
    let indexes: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT indexname
        FROM pg_indexes
        WHERE schemaname = 'public'
        AND indexname LIKE 'idx_%_user_id'
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query indexes");

    let index_names: Vec<String> = indexes.into_iter().map(|(name,)| name).collect();

    for expected in expected_indexes {
        assert!(
            index_names.contains(&expected.to_string()),
            "Index {} should exist. Found: {:?}",
            expected,
            index_names
        );
    }
}

/// MIG-12: Composite indexes exist
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_12_composite_indexes_exist() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    let composite_indexes = vec![
        "idx_tasks_user_project",
        "idx_workspaces_user_task",
        "idx_sessions_user_workspace",
    ];

    let indexes: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT indexname
        FROM pg_indexes
        WHERE schemaname = 'public'
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query indexes");

    let index_names: Vec<String> = indexes.into_iter().map(|(name,)| name).collect();

    for expected in composite_indexes {
        assert!(
            index_names.contains(&expected.to_string()),
            "Composite index {} should exist",
            expected
        );
    }
}

/// MIG-13: NOT NULL constraints on user_id columns
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_13_user_id_not_null_constraints() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    let tables_with_user_id = vec![
        "projects",
        "tasks",
        "workspaces",
        "sessions",
        "execution_processes",
        "repos",
    ];

    // Query all user_id columns
    let nullable_user_ids: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT table_name, is_nullable
        FROM information_schema.columns
        WHERE column_name = 'user_id'
        AND table_schema = 'public'
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query columns");

    for table in tables_with_user_id {
        let col = nullable_user_ids
            .iter()
            .find(|(t, _)| t == table);

        assert!(
            col.is_some(),
            "Table {} should have user_id column",
            table
        );

        let (_, nullable) = col.unwrap();
        assert_eq!(
            nullable, "NO",
            "user_id in {} should be NOT NULL",
            table
        );
    }
}

/// MIG-15: UUID extension enabled
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_15_uuid_extension_enabled() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Check if uuid-ossp extension is installed
    let extension: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT extname
        FROM pg_extension
        WHERE extname = 'uuid-ossp'
        "#,
    )
    .fetch_optional(&service.pool)
    .await
    .expect("Failed to query extensions");

    assert!(
        extension.is_some(),
        "uuid-ossp extension should be installed"
    );

    // Test uuid_generate_v4() works
    let uuid: (uuid::Uuid,) = sqlx::query_as("SELECT uuid_generate_v4()")
        .fetch_one(&service.pool)
        .await
        .expect("uuid_generate_v4() should work");

    assert!(!uuid.0.is_nil(), "Generated UUID should not be nil");
}

/// MIG-16: updated_at trigger on user_configs
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_16_user_configs_trigger() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Check trigger exists
    let trigger: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT trigger_name
        FROM information_schema.triggers
        WHERE event_object_table = 'user_configs'
        AND trigger_name = 'update_user_configs_updated_at'
        "#,
    )
    .fetch_optional(&service.pool)
    .await
    .expect("Failed to query triggers");

    assert!(
        trigger.is_some(),
        "update_user_configs_updated_at trigger should exist"
    );

    // Check function exists
    let function: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT routine_name
        FROM information_schema.routines
        WHERE routine_name = 'update_updated_at_column'
        AND routine_schema = 'public'
        "#,
    )
    .fetch_optional(&service.pool)
    .await
    .expect("Failed to query functions");

    assert!(
        function.is_some(),
        "update_updated_at_column function should exist"
    );
}

/// MIG-17: Initial schema tables created
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_17_initial_schema_tables() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    let expected_tables = vec![
        "projects",
        "repos",
        "project_repos",
        "tasks",
        "workspaces",
        "workspace_repos",
        "sessions",
        "merges",
        "execution_processes",
        "execution_process_logs",
        "execution_process_repo_states",
        "coding_agent_turns",
        "images",
        "task_images",
        "user_configs",
        "pty_sessions",
    ];

    let tables: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT table_name
        FROM information_schema.tables
        WHERE table_schema = 'public'
        AND table_type = 'BASE TABLE'
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query tables");

    let table_names: Vec<String> = tables.into_iter().map(|(name,)| name).collect();

    for expected in expected_tables {
        assert!(
            table_names.contains(&expected.to_string()),
            "Table {} should exist. Found: {:?}",
            expected,
            table_names
        );
    }
}

/// MIG-18: Migration order is correct
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_18_migration_order() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Query _sqlx_migrations table
    let migrations: Vec<(i64, String)> = sqlx::query_as(
        r#"
        SELECT version, description
        FROM _sqlx_migrations
        ORDER BY version ASC
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query migrations");

    // Verify we have at least 5 migrations
    assert!(
        migrations.len() >= 5,
        "Should have at least 5 migrations, found {}",
        migrations.len()
    );

    // Verify versions are in ascending order
    let versions: Vec<i64> = migrations.iter().map(|(v, _)| *v).collect();
    for i in 1..versions.len() {
        assert!(
            versions[i] > versions[i - 1],
            "Migration versions should be ascending"
        );
    }

    // Verify expected migrations are present
    let expected_versions: Vec<i64> = vec![
        20260122000001,
        20260122000002,
        20260122000003,
        20260122000004,
        20260122000005,
    ];

    for expected in expected_versions {
        assert!(
            versions.contains(&expected),
            "Migration version {} should be present",
            expected
        );
    }
}

/// MIG-09: User configs table schema validation
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_09_user_configs_table_schema() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Query for user_configs table columns
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        r#"
        SELECT column_name, data_type, is_nullable
        FROM information_schema.columns
        WHERE table_name = 'user_configs'
        ORDER BY ordinal_position
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query columns");

    // Verify expected columns
    let column_names: Vec<String> = columns.iter().map(|(n, _, _)| n.clone()).collect();
    assert!(
        column_names.contains(&"user_id".to_string()),
        "Should have user_id column"
    );
    assert!(
        column_names.contains(&"config_json".to_string()),
        "Should have config_json column"
    );
    assert!(
        column_names.contains(&"oauth_credentials".to_string()),
        "Should have oauth_credentials column"
    );

    // Verify user_id is primary key (not null in this check)
    let user_id_col = columns.iter().find(|(n, _, _)| n == "user_id");
    assert!(user_id_col.is_some(), "user_id column should exist");

    // Check config_json data type
    let config_json_col = columns.iter().find(|(n, _, _)| n == "config_json");
    if let Some((_, dtype, _)) = config_json_col {
        assert!(
            dtype == "jsonb" || dtype.contains("json"),
            "config_json should be JSONB type"
        );
    }
}

/// MIG-10: PTY sessions table schema validation
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_10_pty_sessions_table_schema() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Query for pty_sessions table columns
    let columns: Vec<(String, String, String)> = sqlx::query_as(
        r#"
        SELECT column_name, data_type, is_nullable
        FROM information_schema.columns
        WHERE table_name = 'pty_sessions'
        ORDER BY ordinal_position
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query columns");

    // Verify expected columns
    let column_names: Vec<String> = columns.iter().map(|(n, _, _)| n.clone()).collect();
    assert!(
        column_names.contains(&"id".to_string()),
        "Should have id column"
    );
    assert!(
        column_names.contains(&"user_id".to_string()),
        "Should have user_id column"
    );
    assert!(
        column_names.contains(&"workspace_id".to_string()),
        "Should have workspace_id column"
    );
    assert!(
        column_names.contains(&"last_activity_at".to_string()),
        "Should have last_activity_at column"
    );

    // Verify workspace_id is nullable (ON DELETE SET NULL)
    let workspace_col = columns.iter().find(|(n, _, _)| n == "workspace_id");
    if let Some((_, _, nullable)) = workspace_col {
        assert_eq!(nullable, "YES", "workspace_id should be nullable");
    }
}

/// MIG-14: Foreign key constraints
#[tokio::test]
#[ignore = "requires running PostgreSQL instance"]
async fn mig_14_foreign_key_constraints() {
    let database_url = get_database_url().expect("DATABASE_URL must be set");
    let service = db::DBServicePg::new_with_url(&database_url, 5)
        .await
        .expect("Failed to create service");

    // Query for foreign key constraints
    let fk_constraints: Vec<(String, String, String, String)> = sqlx::query_as(
        r#"
        SELECT
            tc.table_name,
            kcu.column_name,
            ccu.table_name AS foreign_table_name,
            ccu.column_name AS foreign_column_name
        FROM information_schema.table_constraints AS tc
        JOIN information_schema.key_column_usage AS kcu
            ON tc.constraint_name = kcu.constraint_name
            AND tc.table_schema = kcu.table_schema
        JOIN information_schema.constraint_column_usage AS ccu
            ON ccu.constraint_name = tc.constraint_name
            AND ccu.table_schema = tc.table_schema
        WHERE tc.constraint_type = 'FOREIGN KEY'
        AND tc.table_schema = 'public'
        "#,
    )
    .fetch_all(&service.pool)
    .await
    .expect("Failed to query foreign keys");

    // Verify key relationships
    let has_fk = |table: &str, column: &str, ref_table: &str| -> bool {
        fk_constraints.iter().any(|(t, c, rt, _)| {
            t == table && c == column && rt == ref_table
        })
    };

    // tasks.project_id -> projects.id
    assert!(
        has_fk("tasks", "project_id", "projects"),
        "tasks should have FK to projects"
    );

    // workspaces.task_id -> tasks.id
    assert!(
        has_fk("workspaces", "task_id", "tasks"),
        "workspaces should have FK to tasks"
    );

    // sessions.workspace_id -> workspaces.id
    assert!(
        has_fk("sessions", "workspace_id", "workspaces"),
        "sessions should have FK to workspaces"
    );

    // execution_processes.session_id -> sessions.id
    assert!(
        has_fk("execution_processes", "session_id", "sessions"),
        "execution_processes should have FK to sessions"
    );
}
