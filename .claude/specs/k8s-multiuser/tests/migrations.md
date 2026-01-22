# PostgreSQL Migration Unit Test Cases

## Test File

`crates/db/tests/migrations.rs`

## Test Purpose

This module tests the PostgreSQL database migrations for the multi-user Kubernetes deployment. It validates that:

1. Migrations run successfully on a fresh database
2. Migrations are idempotent (can be run multiple times safely)
3. All expected tables are created with correct schema
4. User isolation indexes are properly created
5. Constraints and triggers are correctly applied

## Test Cases Overview

| Case ID | Feature Description | Test Type |
|---------|-------------------|-----------|
| MIG-01 | Fresh database migration execution | Positive Test |
| MIG-02 | Migration idempotency - run twice | Positive Test |
| MIG-03 | Projects table schema validation | Schema Test |
| MIG-04 | Tasks table schema validation | Schema Test |
| MIG-05 | Workspaces table schema validation | Schema Test |
| MIG-06 | Sessions table schema validation | Schema Test |
| MIG-07 | Execution processes table schema validation | Schema Test |
| MIG-08 | Repos table schema validation | Schema Test |
| MIG-09 | User configs table schema validation | Schema Test |
| MIG-10 | PTY sessions table schema validation | Schema Test |
| MIG-11 | User ID indexes exist | Index Test |
| MIG-12 | Composite indexes exist | Index Test |
| MIG-13 | NOT NULL constraints on user_id columns | Constraint Test |
| MIG-14 | Foreign key constraints | Constraint Test |
| MIG-15 | UUID extension enabled | Setup Test |
| MIG-16 | updated_at trigger on user_configs | Trigger Test |
| MIG-17 | Initial schema tables created | Schema Test |
| MIG-18 | Migration order is correct | Order Test |

## Detailed Test Steps

### MIG-01: Fresh database migration execution

**Test Purpose**: Verify that all migrations run successfully on a fresh PostgreSQL database.

**Test Data Preparation**:
- Create a fresh test database
- Set DATABASE_URL environment variable

**Test Steps**:
1. Connect to a fresh PostgreSQL database
2. Run all migrations using `sqlx::migrate!`
3. Verify no errors occur

**Expected Results**:
- All migrations complete successfully
- No SQL errors returned
- Database is in expected state

---

### MIG-02: Migration idempotency - run twice

**Test Purpose**: Verify that migrations can be run multiple times without error (using IF NOT EXISTS clauses).

**Test Data Preparation**:
- Run migrations on a database
- Keep the same database connection

**Test Steps**:
1. Run all migrations once
2. Run all migrations again
3. Verify no errors occur

**Expected Results**:
- Second migration run completes without errors
- Database state unchanged after second run
- `IF NOT EXISTS` clauses work correctly

---

### MIG-03: Projects table schema validation

**Test Purpose**: Verify projects table has correct columns and types.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query information_schema.columns for projects table
2. Verify columns: id (UUID), user_id (UUID), name (TEXT), etc.
3. Verify user_id is NOT NULL

**Expected Results**:
- All expected columns exist
- Data types are correct
- NOT NULL constraint on user_id

---

### MIG-04: Tasks table schema validation

**Test Purpose**: Verify tasks table has correct columns and constraints.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query information_schema for tasks table schema
2. Verify status column CHECK constraint
3. Verify foreign key to projects

**Expected Results**:
- Status CHECK constraint includes: 'todo', 'inprogress', 'done', 'cancelled', 'inreview'
- project_id references projects(id)

---

### MIG-05: Workspaces table schema validation

**Test Purpose**: Verify workspaces table has correct schema.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query schema for workspaces table
2. Verify columns: id, user_id, task_id, branch, container_ref, etc.
3. Verify foreign key to tasks

**Expected Results**:
- All columns exist with correct types
- Foreign key constraint on task_id

---

### MIG-06: Sessions table schema validation

**Test Purpose**: Verify sessions table schema.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query schema for sessions table
2. Verify user_id column exists
3. Verify workspace_id foreign key

**Expected Results**:
- user_id column is NOT NULL
- workspace_id references workspaces(id)

---

### MIG-07: Execution processes table schema validation

**Test Purpose**: Verify execution_processes table has correct schema including CHECK constraints.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query schema for execution_processes table
2. Verify run_reason CHECK constraint
3. Verify status CHECK constraint

**Expected Results**:
- run_reason IN ('setupscript', 'codingagent', 'devserver', 'cleanupscript')
- status IN ('running', 'completed', 'failed', 'killed')

---

### MIG-08: Repos table schema validation

**Test Purpose**: Verify repos table schema and unique constraint.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query schema for repos table
2. Verify UNIQUE constraint on (user_id, path)

**Expected Results**:
- Unique constraint prevents duplicate paths per user
- Path column is TEXT type

---

### MIG-09: User configs table schema validation

**Test Purpose**: Verify user_configs table has correct schema for configuration storage.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query schema for user_configs table
2. Verify columns: user_id (PK), config_json (JSONB), oauth_credentials (BYTEA)
3. Verify default value for config_json

**Expected Results**:
- user_id is PRIMARY KEY
- config_json has DEFAULT '{}'
- oauth_credentials is BYTEA for encrypted data

---

### MIG-10: PTY sessions table schema validation

**Test Purpose**: Verify pty_sessions table schema for terminal session tracking.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query schema for pty_sessions table
2. Verify columns: id, user_id, workspace_id, created_at, last_activity_at
3. Verify workspace_id allows NULL (ON DELETE SET NULL)

**Expected Results**:
- workspace_id is nullable
- last_activity_at exists for idle cleanup

---

### MIG-11: User ID indexes exist

**Test Purpose**: Verify all user_id indexes are created for efficient filtering.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query pg_indexes for user_id indexes
2. Verify indexes exist for: projects, repos, tasks, workspaces, sessions, execution_processes

**Expected Results**:
- idx_projects_user_id exists
- idx_repos_user_id exists
- idx_tasks_user_id exists
- idx_workspaces_user_id exists
- idx_sessions_user_id exists
- idx_execution_processes_user_id exists

---

### MIG-12: Composite indexes exist

**Test Purpose**: Verify composite indexes for common query patterns.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query pg_indexes for composite indexes
2. Verify: idx_tasks_user_project, idx_workspaces_user_task, idx_sessions_user_workspace

**Expected Results**:
- All composite indexes exist
- Index definitions include correct columns

---

### MIG-13: NOT NULL constraints on user_id columns

**Test Purpose**: Verify all user_id columns have NOT NULL constraints.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query information_schema.columns for user_id columns
2. Verify is_nullable = 'NO' for all

**Expected Results**:
- All user_id columns are NOT NULL
- Tables: projects, tasks, workspaces, sessions, execution_processes, repos

---

### MIG-14: Foreign key constraints

**Test Purpose**: Verify foreign key relationships are correct.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query information_schema.table_constraints for foreign keys
2. Verify key relationships between tables

**Expected Results**:
- tasks.project_id -> projects.id
- workspaces.task_id -> tasks.id
- sessions.workspace_id -> workspaces.id
- execution_processes.session_id -> sessions.id

---

### MIG-15: UUID extension enabled

**Test Purpose**: Verify uuid-ossp extension is installed.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query pg_extension for uuid-ossp
2. Verify uuid_generate_v4() function exists

**Expected Results**:
- Extension is installed
- UUID generation works

---

### MIG-16: updated_at trigger on user_configs

**Test Purpose**: Verify the automatic updated_at trigger on user_configs table.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query information_schema.triggers for user_configs
2. Verify update_user_configs_updated_at trigger exists
3. Verify it calls update_updated_at_column function

**Expected Results**:
- Trigger exists and is BEFORE UPDATE
- Function update_updated_at_column() exists

---

### MIG-17: Initial schema tables created

**Test Purpose**: Verify all tables from initial schema migration exist.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query information_schema.tables
2. Verify all expected tables exist

**Expected Results**:
- Tables exist: projects, repos, project_repos, tasks, workspaces, workspace_repos, sessions, merges, execution_processes, execution_process_logs, execution_process_repo_states, coding_agent_turns, images, task_images

---

### MIG-18: Migration order is correct

**Test Purpose**: Verify migrations are applied in correct order.

**Test Data Preparation**:
- Database with migrations applied

**Test Steps**:
1. Query _sqlx_migrations table
2. Verify version numbers are sequential
3. Verify all expected migrations are recorded

**Expected Results**:
- 5 migrations recorded
- Versions: 20260122000001 through 20260122000005
- All migrations have success=true

---

## Test Considerations

### Mock Strategy

- Tests use a real PostgreSQL database (not mocked)
- Tests run in an isolated test database
- Database is cleaned up before each test
- Connection pooling is tested with real connections

### Boundary Conditions

- Empty database before migrations
- Partial migration failure recovery
- Schema with pre-existing conflicting objects
- Large-scale data migration performance

### Asynchronous Operations

- All database operations are async
- Tests use `#[tokio::test]`
- Connection pool initialization is async

### Environment Setup

Tests require:
- `DATABASE_URL` environment variable set to a PostgreSQL connection string
- PostgreSQL server running and accessible
- Permissions to create/drop tables and indexes

Run tests with:
```bash
cargo test -p db --test migrations -- --ignored --test-threads=1
```

### Cleanup Strategy

- Tests should create unique schemas or databases
- Use transactions for rollback when possible
- Clean up test data after assertions
