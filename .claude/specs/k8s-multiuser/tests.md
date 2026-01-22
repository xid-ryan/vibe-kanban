# K8s Multi-User Deployment Test Cases

## Test File Structure

```
crates/
  server/
    src/
      middleware/
        auth.rs          # Unit tests (inline #[cfg(test)])
    tests/
      user_isolation.rs  # Integration tests
      websocket_auth.rs  # WebSocket integration tests
  services/
    tests/
      security_paths.rs  # Path traversal security tests
  db/
    tests/
      migrations.rs      # Database migration tests
  local-deployment/
    src/
      pty.rs            # Unit tests (inline #[cfg(test)])
      container.rs      # Unit tests (inline #[cfg(test)])
tests/
  load/                  # Load testing scripts
```

## Test Purpose

This document defines test cases for the K8s Multi-User Deployment feature, ensuring:
- Complete user isolation across all resources
- Secure JWT-based authentication
- Proper path validation preventing unauthorized access
- Database migration correctness
- WebSocket session security
- Performance under concurrent user load

---

## Test Cases Overview

### Unit Tests

| Case ID  | Feature Description                           | Test Type       | Requirement ID |
|----------|-----------------------------------------------|-----------------|----------------|
| AUTH-01  | JWT validation with valid token               | Positive Test   | 1.3            |
| AUTH-02  | JWT validation with expired token             | Error Test      | 1.2            |
| AUTH-03  | JWT validation with invalid signature         | Error Test      | 1.2            |
| AUTH-04  | JWT validation with missing user_id claim     | Error Test      | 1.6            |
| AUTH-05  | JWT extraction from Authorization header      | Positive Test   | 1.1            |
| AUTH-06  | Missing Authorization header returns 401      | Error Test      | 1.1            |
| AUTH-07  | UserContext propagation to request extensions | Positive Test   | 1.4            |
| PATH-01  | Path validation within user workspace         | Positive Test   | 3.3            |
| PATH-02  | Path traversal with ".." rejected             | Security Test   | 3.4, 8.3       |
| PATH-03  | Symlink following validated                   | Security Test   | 3.4            |
| PATH-04  | URL-encoded path traversal rejected           | Security Test   | 8.3            |
| PATH-05  | Absolute path outside workspace rejected      | Security Test   | 3.4            |
| CFG-01   | Load configuration for existing user          | Positive Test   | 4.1            |
| CFG-02   | Load configuration returns defaults for new   | Positive Test   | 4.3            |
| CFG-03   | Save configuration upserts correctly          | Positive Test   | 4.2            |
| CFG-04   | Credential encryption round-trip              | Positive Test   | 4.5, 4.6       |
| CFG-05   | Credential decryption with wrong key fails    | Error Test      | 12.2           |
| PTY-01   | Session creation with valid working dir       | Positive Test   | 5.1            |
| PTY-02   | Session creation outside workspace rejected   | Error Test      | 5.1            |
| PTY-03   | Session ownership validation                  | Positive Test   | 5.6            |
| PTY-04   | List sessions returns only user's sessions    | Positive Test   | 5.4            |
| PTY-05   | Session timeout and cleanup                   | Positive Test   | 5.5            |

### Integration Tests

| Case ID  | Feature Description                           | Test Type       | Requirement ID |
|----------|-----------------------------------------------|-----------------|----------------|
| INT-01   | User A cannot see User B's projects           | Isolation Test  | 2.5, 2.8       |
| INT-02   | User A cannot see User B's tasks              | Isolation Test  | 2.5, 2.8       |
| INT-03   | User A cannot see User B's workspaces         | Isolation Test  | 2.5, 2.8       |
| INT-04   | Cross-user project access returns 404         | Security Test   | 2.8            |
| INT-05   | Cross-user task access returns 404            | Security Test   | 2.8            |
| INT-06   | Cross-user workspace access returns 404       | Security Test   | 2.8            |
| INT-07   | Workspace creation uses correct user path     | Positive Test   | 3.1, 3.2       |
| INT-08   | PTY session isolated between users            | Isolation Test  | 5.3, 5.6       |
| INT-09   | Process ownership isolated between users      | Isolation Test  | 7.2, 7.4       |
| INT-10   | Git operations restricted to user workspace   | Isolation Test  | 6.1, 6.3       |
| INT-11   | Filesystem listing restricted to user dir     | Isolation Test  | 8.1            |
| INT-12   | Config operations isolated per user           | Isolation Test  | 4.1, 4.2       |

### WebSocket Tests

| Case ID  | Feature Description                           | Test Type       | Requirement ID |
|----------|-----------------------------------------------|-----------------|----------------|
| WS-01    | WebSocket connection requires valid JWT       | Security Test   | 10.1           |
| WS-02    | WebSocket with invalid JWT rejected           | Error Test      | 10.5           |
| WS-03    | PTY operations validate session ownership     | Security Test   | 10.5           |
| WS-04    | Reconnection within grace period succeeds     | Positive Test   | 10.2, 10.3     |
| WS-05    | Session state preserved during reconnection   | Positive Test   | 10.3           |

### Migration Tests

| Case ID  | Feature Description                           | Test Type       | Requirement ID |
|----------|-----------------------------------------------|-----------------|----------------|
| MIG-01   | Fresh database migration succeeds             | Positive Test   | 2.3            |
| MIG-02   | Migrations are idempotent                     | Positive Test   | NFR-Maintain   |
| MIG-03   | user_id columns added to all tables           | Schema Test     | 2.4            |
| MIG-04   | Indexes created correctly                     | Schema Test     | 2.7            |
| MIG-05   | user_configs table created                    | Schema Test     | 4.2            |
| MIG-06   | pty_sessions table created                    | Schema Test     | 5.3            |

### Security Tests

| Case ID  | Feature Description                           | Test Type       | Requirement ID |
|----------|-----------------------------------------------|-----------------|----------------|
| SEC-01   | User enumeration prevented (404 vs 403)       | Security Test   | 2.8            |
| SEC-02   | Rate limiting on failed auth attempts         | Security Test   | 12.4           |
| SEC-03   | Unauthorized access logged with context       | Audit Test      | 12.7           |
| SEC-04   | OAuth tokens encrypted at rest                | Security Test   | 12.2           |
| SEC-05   | Session hijacking via stolen session ID       | Security Test   | 5.6            |

### Load Tests

| Case ID  | Feature Description                           | Test Type       | Requirement ID |
|----------|-----------------------------------------------|-----------------|----------------|
| LOAD-01  | 100 concurrent users simulation               | Performance     | NFR-Perf-1     |
| LOAD-02  | API response time < 200ms at p95              | Performance     | NFR-Perf-2     |
| LOAD-03  | PTY input latency < 50ms                      | Performance     | NFR-Perf-3     |
| LOAD-04  | Database connection pool under load           | Performance     | NFR-Avail-3    |

---

## Detailed Test Steps

### AUTH-01: JWT Validation with Valid Token

**Test Purpose**: Verify that a valid JWT token with correct signature and claims is accepted.

**Test Data Preparation**:
- Generate a valid JWT token with:
  - `sub` claim: UUID `550e8400-e29b-41d4-a716-446655440000`
  - `email` claim: `test@example.com`
  - `exp` claim: current time + 1 hour
  - Sign with test `JWT_SECRET`

**Test Steps**:
1. Create JWT with valid claims and signature
2. Call `verify_jwt(token)` function
3. Verify UserContext is returned with correct user_id and email

**Expected Results**:
- Function returns `Ok(UserContext)`
- `user_context.user_id` equals UUID from `sub` claim
- `user_context.email` equals `Some("test@example.com")`

---

### AUTH-02: JWT Validation with Expired Token

**Test Purpose**: Verify that expired JWT tokens are rejected with 401.

**Test Data Preparation**:
- Generate JWT token with `exp` claim set to past time

**Test Steps**:
1. Create JWT with expired timestamp
2. Call `verify_jwt(token)` function
3. Verify error is returned

**Expected Results**:
- Function returns `Err(AuthError::ExpiredToken)`
- Error converts to HTTP 401 response

---

### AUTH-03: JWT Validation with Invalid Signature

**Test Purpose**: Verify tokens signed with wrong secret are rejected.

**Test Data Preparation**:
- Generate JWT signed with different secret than `JWT_SECRET`

**Test Steps**:
1. Create JWT signed with wrong secret
2. Call `verify_jwt(token)` function
3. Verify error is returned

**Expected Results**:
- Function returns `Err(AuthError::InvalidToken)`
- Error converts to HTTP 401 response

---

### AUTH-04: JWT Validation with Missing user_id Claim

**Test Purpose**: Verify tokens without `sub` claim are rejected with 400.

**Test Data Preparation**:
- Generate valid JWT without `sub` claim

**Test Steps**:
1. Create JWT without `sub` claim
2. Call `verify_jwt(token)` function
3. Verify error indicates missing claim

**Expected Results**:
- Function returns `Err(AuthError::MissingClaim("sub"))`
- Error converts to HTTP 400 response

---

### AUTH-05: JWT Extraction from Authorization Header

**Test Purpose**: Verify Bearer token is correctly extracted from header.

**Test Data Preparation**:
- Create mock HTTP request with `Authorization: Bearer <token>`

**Test Steps**:
1. Create request with Authorization header
2. Call `require_auth` middleware
3. Verify token is extracted and validated

**Expected Results**:
- Middleware extracts token from `Bearer ` prefix
- UserContext is attached to request extensions

---

### AUTH-06: Missing Authorization Header Returns 401

**Test Purpose**: Verify requests without Authorization header are rejected.

**Test Data Preparation**:
- Create mock HTTP request without Authorization header

**Test Steps**:
1. Create request without Authorization header
2. Call `require_auth` middleware
3. Verify 401 response

**Expected Results**:
- Middleware returns 401 Unauthorized
- Response body contains `{"error": "unauthorized"}`

---

### AUTH-07: UserContext Propagation to Request Extensions

**Test Purpose**: Verify UserContext is accessible in downstream handlers.

**Test Data Preparation**:
- Valid JWT token
- Mock request with Authorization header

**Test Steps**:
1. Create request with valid JWT
2. Pass through `require_auth` middleware
3. In route handler, call `extract_user_context(request)`
4. Verify UserContext is returned

**Expected Results**:
- `extract_user_context` returns `Ok(&UserContext)`
- UserContext contains correct user_id from token

---

### PATH-01: Path Validation Within User Workspace

**Test Purpose**: Verify paths within user's workspace are accepted.

**Test Data Preparation**:
- User ID: `550e8400-e29b-41d4-a716-446655440000`
- Base path: `/workspaces/550e8400-e29b-41d4-a716-446655440000/`
- Test path: `/workspaces/550e8400-e29b-41d4-a716-446655440000/project1/src/main.rs`

**Test Steps**:
1. Call `validate_user_path(user_id, path)`
2. Verify path is accepted

**Expected Results**:
- Function returns `Ok(PathBuf)` with canonicalized path
- No error returned

---

### PATH-02: Path Traversal with ".." Rejected

**Test Purpose**: Verify path traversal attempts are blocked.

**Test Data Preparation**:
- User ID: `550e8400-e29b-41d4-a716-446655440000`
- Malicious path: `/workspaces/550e8400-e29b-41d4-a716-446655440000/../other-user/secrets`

**Test Steps**:
1. Call `validate_user_path(user_id, malicious_path)`
2. Verify error is returned

**Expected Results**:
- Function returns `Err(WorkspaceError::Unauthorized)` or `Err(SecurityError::PathOutsideBoundary)`
- Path traversal is blocked

---

### PATH-03: Symlink Following Validated

**Test Purpose**: Verify symlinks resolving outside workspace are rejected.

**Test Data Preparation**:
- Create symlink inside user workspace pointing to `/etc/passwd`
- User ID: `550e8400-e29b-41d4-a716-446655440000`

**Test Steps**:
1. Create test symlink: `/workspaces/{user_id}/link` -> `/etc/passwd`
2. Call `validate_user_path(user_id, "/workspaces/{user_id}/link")`
3. Verify error after canonicalization

**Expected Results**:
- Function returns error after resolving symlink
- Symlink to outside location is rejected

---

### PATH-04: URL-Encoded Path Traversal Rejected

**Test Purpose**: Verify URL-encoded path traversal is blocked.

**Test Data Preparation**:
- Malicious path: `/workspaces/{user_id}/%2e%2e/other-user`

**Test Steps**:
1. URL-decode the path
2. Call `validate_user_path(user_id, decoded_path)`
3. Verify error is returned

**Expected Results**:
- Function properly decodes and validates path
- Encoded traversal is rejected

---

### PATH-05: Absolute Path Outside Workspace Rejected

**Test Purpose**: Verify absolute paths not starting with user's workspace are rejected.

**Test Data Preparation**:
- User ID: `550e8400-e29b-41d4-a716-446655440000`
- Malicious path: `/etc/passwd`

**Test Steps**:
1. Call `validate_user_path(user_id, "/etc/passwd")`
2. Verify error is returned

**Expected Results**:
- Function returns `Err(SecurityError::PathOutsideBoundary)`
- Absolute path outside workspace rejected

---

### CFG-01: Load Configuration for Existing User

**Test Purpose**: Verify configuration is loaded correctly from database.

**Test Data Preparation**:
- Insert config record for user in database
- User ID: `550e8400-e29b-41d4-a716-446655440000`

**Test Steps**:
1. Insert config into `user_configs` table
2. Call `config_service.load_config(user_id)`
3. Verify config matches stored data

**Expected Results**:
- Function returns stored Config struct
- All fields match database values

---

### CFG-02: Load Configuration Returns Defaults for New User

**Test Purpose**: Verify sensible defaults returned for users without config.

**Test Data Preparation**:
- User ID: `new-user-id` (no existing config)

**Test Steps**:
1. Call `config_service.load_config(new_user_id)`
2. Verify default Config is returned

**Expected Results**:
- Function returns default Config values
- No database error

---

### CFG-03: Save Configuration Upserts Correctly

**Test Purpose**: Verify save creates or updates config record.

**Test Data Preparation**:
- User ID and Config struct

**Test Steps**:
1. Call `save_config(user_id, config)` for new user
2. Verify record created
3. Modify config and call `save_config` again
4. Verify record updated, not duplicated

**Expected Results**:
- First call creates new record
- Second call updates existing record
- `updated_at` timestamp changes

---

### CFG-04: Credential Encryption Round-Trip

**Test Purpose**: Verify credentials can be encrypted and decrypted.

**Test Data Preparation**:
- Sample OAuth credentials
- Encryption key

**Test Steps**:
1. Create Credentials struct with test data
2. Call `encrypt_credentials(creds)`
3. Call `decrypt_credentials(encrypted)`
4. Verify decrypted equals original

**Expected Results**:
- Encrypted data is not plaintext
- Decrypted data matches original
- Different encryptions produce different ciphertext (random nonce)

---

### CFG-05: Credential Decryption with Wrong Key Fails

**Test Purpose**: Verify decryption fails with incorrect key.

**Test Data Preparation**:
- Credentials encrypted with key A
- Attempt decryption with key B

**Test Steps**:
1. Encrypt credentials with encryption_key_a
2. Create ConfigService with encryption_key_b
3. Attempt `decrypt_credentials`
4. Verify error

**Expected Results**:
- Function returns decryption error
- No sensitive data leaked in error message

---

### PTY-01: Session Creation with Valid Working Dir

**Test Purpose**: Verify PTY session created in user's workspace.

**Test Data Preparation**:
- User ID: `550e8400-e29b-41d4-a716-446655440000`
- Working dir: `/workspaces/{user_id}/project1`

**Test Steps**:
1. Call `create_session(user_id, working_dir, cols, rows)`
2. Verify session created
3. Verify session associated with user_id

**Expected Results**:
- Function returns `Ok((session_id, output_rx))`
- Session stored with correct user_id ownership

---

### PTY-02: Session Creation Outside Workspace Rejected

**Test Purpose**: Verify PTY session cannot be created outside user's workspace.

**Test Data Preparation**:
- User ID: `550e8400-e29b-41d4-a716-446655440000`
- Invalid working dir: `/tmp/malicious`

**Test Steps**:
1. Call `create_session(user_id, invalid_dir, cols, rows)`
2. Verify error returned

**Expected Results**:
- Function returns error (path validation failure)
- No session created

---

### PTY-03: Session Ownership Validation

**Test Purpose**: Verify only session owner can interact with session.

**Test Data Preparation**:
- User A creates session
- User B attempts to write to session

**Test Steps**:
1. User A calls `create_session`
2. Get session_id
3. User B calls `validate_session_ownership(session_id, user_b_id)`
4. Verify ownership check fails

**Expected Results**:
- Ownership validation returns error for wrong user
- Error type is `NotFound` (not `Forbidden` - prevents enumeration)

---

### PTY-04: List Sessions Returns Only User's Sessions

**Test Purpose**: Verify session listing is user-scoped.

**Test Data Preparation**:
- User A creates 2 sessions
- User B creates 1 session

**Test Steps**:
1. User A creates sessions
2. User B creates session
3. Call `list_user_sessions(user_a_id)`
4. Verify only User A's sessions returned

**Expected Results**:
- Returns 2 sessions for User A
- User B's session not included

---

### PTY-05: Session Timeout and Cleanup

**Test Purpose**: Verify idle sessions are cleaned up after timeout.

**Test Data Preparation**:
- Create session with last_activity in past

**Test Steps**:
1. Create session
2. Manually set `last_activity_at` to 35 minutes ago
3. Call `cleanup_idle_sessions(Duration::minutes(30))`
4. Verify session removed

**Expected Results**:
- Session cleaned up after timeout
- Session no longer accessible

---

### INT-01: User A Cannot See User B's Projects

**Test Purpose**: Verify complete project isolation between users.

**Test Data Preparation**:
- User A JWT token
- User B JWT token
- User A creates project "Project Alpha"

**Test Steps**:
1. User A: POST /api/projects with name "Project Alpha"
2. User B: GET /api/projects
3. Verify User B's response is empty array

**Expected Results**:
- User B receives `[]` (empty projects list)
- User A's project not visible to User B

---

### INT-02: User A Cannot See User B's Tasks

**Test Purpose**: Verify complete task isolation between users.

**Test Data Preparation**:
- User A creates project and task
- User B JWT token

**Test Steps**:
1. User A: Create project and task
2. User B: GET /api/tasks
3. Verify User B's response is empty

**Expected Results**:
- User B sees no tasks
- User A's tasks not accessible

---

### INT-03: User A Cannot See User B's Workspaces

**Test Purpose**: Verify workspace isolation between users.

**Test Data Preparation**:
- User A creates workspace
- User B JWT token

**Test Steps**:
1. User A: Create workspace
2. User B: GET /api/sessions (workspaces)
3. Verify User B sees no workspaces

**Expected Results**:
- User B receives empty workspace list
- User A's workspace paths not exposed

---

### INT-04: Cross-User Project Access Returns 404

**Test Purpose**: Verify accessing another user's project returns 404 (not 403).

**Test Data Preparation**:
- User A creates project with known ID
- User B JWT token

**Test Steps**:
1. User A: POST /api/projects, capture project_id
2. User B: GET /api/projects/{project_id}
3. Verify 404 response

**Expected Results**:
- Response status: 404 Not Found
- Response body: `{"error": "not_found"}`
- No indication project exists (prevents enumeration)

---

### INT-05: Cross-User Task Access Returns 404

**Test Purpose**: Verify accessing another user's task returns 404.

**Test Data Preparation**:
- User A creates task with known ID
- User B JWT token

**Test Steps**:
1. User A: Create task, capture task_id
2. User B: GET /api/tasks/{task_id}
3. Verify 404 response

**Expected Results**:
- Response status: 404 Not Found
- No information about task existence

---

### INT-06: Cross-User Workspace Access Returns 404

**Test Purpose**: Verify accessing another user's workspace returns 404.

**Test Data Preparation**:
- User A creates workspace
- User B JWT token

**Test Steps**:
1. User A: Create workspace, capture workspace_id
2. User B: GET /api/sessions/{workspace_id}
3. Verify 404 response

**Expected Results**:
- Response status: 404 Not Found
- Workspace path not revealed

---

### INT-07: Workspace Creation Uses Correct User Path

**Test Purpose**: Verify workspace created under user's directory.

**Test Data Preparation**:
- User A JWT token with user_id
- Workspace creation request

**Test Steps**:
1. User A: POST /api/sessions to create workspace
2. Retrieve workspace details
3. Verify `container_ref` starts with `/workspaces/{user_a_id}/`

**Expected Results**:
- Workspace path: `/workspaces/{user_id}/{workspace_name}`
- Path contains correct user_id

---

### INT-08: PTY Session Isolated Between Users

**Test Purpose**: Verify WebSocket PTY sessions are user-isolated.

**Test Data Preparation**:
- User A WebSocket connection
- User B WebSocket connection

**Test Steps**:
1. User A: Connect to /api/terminal, create session
2. User A: Send command "echo $HOME"
3. User B: Attempt to write to User A's session
4. Verify User B's attempt fails

**Expected Results**:
- User A receives command output
- User B receives error for unauthorized session access

---

### INT-09: Process Ownership Isolated Between Users

**Test Purpose**: Verify AI agent processes are user-scoped.

**Test Data Preparation**:
- User A starts container/process
- User B JWT token

**Test Steps**:
1. User A: Start AI agent process
2. User A: GET /api/execution_processes (see process)
3. User B: GET /api/execution_processes
4. Verify User B sees empty list

**Expected Results**:
- User A sees own process
- User B sees no processes

---

### INT-10: Git Operations Restricted to User Workspace

**Test Purpose**: Verify Git operations cannot access outside user's workspace.

**Test Data Preparation**:
- User A workspace with Git repo
- Attempt Git operation outside workspace

**Test Steps**:
1. Create Git repo in User A's workspace
2. Attempt to open repo path outside workspace
3. Verify error returned

**Expected Results**:
- Git operation fails with path validation error
- No access to external repositories

---

### INT-11: Filesystem Listing Restricted to User Dir

**Test Purpose**: Verify filesystem API only lists user's files.

**Test Data Preparation**:
- User A workspace with files
- User B JWT token

**Test Steps**:
1. User A: Create files in workspace
2. User B: GET /api/filesystem/list?path=/workspaces/{user_a_id}
3. Verify User B receives error

**Expected Results**:
- User B cannot list User A's directory
- Path validation rejects cross-user access

---

### INT-12: Config Operations Isolated Per User

**Test Purpose**: Verify configuration is user-specific.

**Test Data Preparation**:
- User A config with specific settings
- User B JWT token

**Test Steps**:
1. User A: PUT /api/config with custom settings
2. User B: GET /api/config
3. Verify User B gets default config, not User A's

**Expected Results**:
- User B receives default configuration
- User A's settings not accessible to User B

---

### WS-01: WebSocket Connection Requires Valid JWT

**Test Purpose**: Verify WebSocket upgrade requires authentication.

**Test Data Preparation**:
- Valid JWT token
- Invalid/missing JWT token

**Test Steps**:
1. Attempt WebSocket connect to /api/terminal without token
2. Verify connection rejected
3. Attempt with valid token
4. Verify connection accepted

**Expected Results**:
- Connection without token: 401 or connection rejected
- Connection with valid token: WebSocket established

---

### WS-02: WebSocket with Invalid JWT Rejected

**Test Purpose**: Verify WebSocket rejects invalid tokens.

**Test Data Preparation**:
- Expired JWT token
- Malformed JWT token

**Test Steps**:
1. Attempt WebSocket connect with expired token
2. Verify connection rejected
3. Attempt with malformed token
4. Verify connection rejected

**Expected Results**:
- Both attempts rejected with 401

---

### WS-03: PTY Operations Validate Session Ownership

**Test Purpose**: Verify each WebSocket message validates session ownership.

**Test Data Preparation**:
- User A WebSocket with session
- User B attempts to send to User A's session

**Test Steps**:
1. User A creates session, gets session_id
2. User B connects via WebSocket
3. User B sends message with User A's session_id
4. Verify rejection

**Expected Results**:
- User B receives error: session not found
- User A's session unaffected

---

### WS-04: Reconnection Within Grace Period Succeeds

**Test Purpose**: Verify session state preserved during brief disconnection.

**Test Data Preparation**:
- User A with active PTY session
- Configure 5-minute grace period

**Test Steps**:
1. User A creates PTY session
2. Disconnect WebSocket
3. Wait 1 minute
4. Reconnect and resume session
5. Verify session state preserved

**Expected Results**:
- Reconnection successful
- Session continues from previous state

---

### WS-05: Session State Preserved During Reconnection

**Test Purpose**: Verify terminal history available after reconnect.

**Test Data Preparation**:
- User A with active session and command history

**Test Steps**:
1. User A creates session
2. Run commands: `echo "test1"`, `echo "test2"`
3. Disconnect
4. Reconnect within grace period
5. Verify session still active

**Expected Results**:
- Session remains accessible
- New commands work after reconnection

---

### MIG-01: Fresh Database Migration Succeeds

**Test Purpose**: Verify migrations run on empty database.

**Test Data Preparation**:
- Empty PostgreSQL database

**Test Steps**:
1. Create empty database
2. Run all migrations
3. Verify all tables created

**Expected Results**:
- No migration errors
- All expected tables exist
- All columns present

---

### MIG-02: Migrations Are Idempotent

**Test Purpose**: Verify running migrations twice causes no errors.

**Test Data Preparation**:
- Database with migrations already run

**Test Steps**:
1. Run migrations (first time)
2. Run migrations (second time)
3. Verify no errors

**Expected Results**:
- Both runs complete successfully
- No duplicate table/column errors

---

### MIG-03: user_id Columns Added to All Tables

**Test Purpose**: Verify all required tables have user_id column.

**Test Data Preparation**:
- Database after migration

**Test Steps**:
1. Run migrations
2. Query information_schema for each table
3. Verify user_id column exists with UUID type

**Expected Results**:
- Tables with user_id: projects, tasks, workspaces, sessions, execution_processes, repos
- Column type: UUID NOT NULL

---

### MIG-04: Indexes Created Correctly

**Test Purpose**: Verify performance indexes exist.

**Test Data Preparation**:
- Database after migration

**Test Steps**:
1. Run migrations
2. Query pg_indexes for expected indexes
3. Verify all indexes exist

**Expected Results**:
- Single-column indexes: idx_projects_user_id, idx_tasks_user_id, etc.
- Composite indexes: idx_tasks_user_project, idx_workspaces_user_task

---

### MIG-05: user_configs Table Created

**Test Purpose**: Verify user configuration table structure.

**Test Data Preparation**:
- Database after migration

**Test Steps**:
1. Run migrations
2. Query user_configs table structure
3. Verify columns

**Expected Results**:
- Columns: user_id (PK), config_json (JSONB), oauth_credentials (BYTEA), created_at, updated_at
- user_id is primary key

---

### MIG-06: pty_sessions Table Created

**Test Purpose**: Verify PTY session tracking table structure.

**Test Data Preparation**:
- Database after migration

**Test Steps**:
1. Run migrations
2. Query pty_sessions table structure
3. Verify columns and indexes

**Expected Results**:
- Columns: id, user_id, workspace_id, created_at, last_activity_at
- Indexes: idx_pty_sessions_user, idx_pty_sessions_activity

---

### SEC-01: User Enumeration Prevented (404 vs 403)

**Test Purpose**: Verify unauthorized access returns 404, not 403.

**Test Data Preparation**:
- User A creates resource with ID
- User B attempts access

**Test Steps**:
1. User A creates project
2. User B: GET /api/projects/{user_a_project_id}
3. User B: GET /api/projects/{nonexistent_id}
4. Compare responses

**Expected Results**:
- Both return 404 Not Found
- Responses are identical (no enumeration possible)

---

### SEC-02: Rate Limiting on Failed Auth Attempts

**Test Purpose**: Verify rate limiting prevents brute force.

**Test Data Preparation**:
- Invalid JWT tokens

**Test Steps**:
1. Send 10 requests with invalid tokens in rapid succession
2. Verify responses
3. Check if rate limiting triggered

**Expected Results**:
- After threshold, receive 429 Too Many Requests
- Rate limit logged

---

### SEC-03: Unauthorized Access Logged with Context

**Test Purpose**: Verify security events are logged for audit.

**Test Data Preparation**:
- User B attempts to access User A's resource

**Test Steps**:
1. User B: Attempt cross-user access
2. Check application logs
3. Verify log contains context

**Expected Results**:
- Log entry contains: user_id, attempted action, resource ID, timestamp
- Log level: WARN or higher

---

### SEC-04: OAuth Tokens Encrypted at Rest

**Test Purpose**: Verify OAuth credentials not stored in plaintext.

**Test Data Preparation**:
- User saves OAuth credentials

**Test Steps**:
1. Save credentials via ConfigService
2. Query raw database value
3. Verify value is encrypted

**Expected Results**:
- Database column contains binary data, not JSON
- Cannot decode without encryption key

---

### SEC-05: Session Hijacking via Stolen Session ID

**Test Purpose**: Verify session ID alone is insufficient for access.

**Test Data Preparation**:
- User A creates PTY session
- User B has User A's session_id (but different JWT)

**Test Steps**:
1. User A creates session, gets session_id
2. User B attempts to use session_id in requests
3. Verify User B cannot access session

**Expected Results**:
- Session ownership checked against JWT user_id
- User B receives session not found error

---

### LOAD-01: 100 Concurrent Users Simulation

**Test Purpose**: Verify system handles 100 concurrent users.

**Test Data Preparation**:
- 100 unique JWT tokens
- Concurrent request script

**Test Steps**:
1. Generate 100 test users
2. Each user: Create project, list projects
3. Run all concurrently
4. Measure success rate and timing

**Expected Results**:
- >99% success rate
- No deadlocks or crashes
- System remains responsive

---

### LOAD-02: API Response Time < 200ms at p95

**Test Purpose**: Verify API performance meets SLA.

**Test Data Preparation**:
- Load test with typical operations

**Test Steps**:
1. Run load test with mixed operations
2. Collect response time metrics
3. Calculate p95 latency

**Expected Results**:
- p95 response time < 200ms
- No requests timeout

---

### LOAD-03: PTY Input Latency < 50ms

**Test Purpose**: Verify terminal responsiveness under load.

**Test Data Preparation**:
- Multiple concurrent PTY sessions

**Test Steps**:
1. Create 50 concurrent PTY sessions
2. Send input to each
3. Measure input-to-output latency

**Expected Results**:
- p95 latency < 50ms
- No dropped keystrokes

---

### LOAD-04: Database Connection Pool Under Load

**Test Purpose**: Verify connection pool handles concurrent requests.

**Test Data Preparation**:
- High-concurrency database operations

**Test Steps**:
1. Send 200 concurrent database queries
2. Monitor connection pool metrics
3. Verify no connection exhaustion

**Expected Results**:
- All queries complete
- Connection pool returns connections properly
- No "too many connections" errors

---

## Test Considerations

### Mock Strategy

**JWT Validation**:
- Use `jsonwebtoken` crate to generate test tokens
- Configure test `JWT_SECRET` environment variable
- Create helper function `create_test_jwt(user_id: &str) -> String`

**Database**:
- Use test PostgreSQL instance or testcontainers
- Each test uses unique user_id for isolation
- Clean up test data after each test

**Filesystem**:
- Use `tempfile::TempDir` for isolated test directories
- Create `/workspaces/{user_id}` structure in temp dir
- Clean up automatically on test completion

**WebSocket**:
- Use `tokio-tungstenite` for test WebSocket clients
- Mock authentication handshake in upgrade

### Boundary Conditions

1. **Empty user_id**: UUID with all zeros
2. **Maximum path length**: Test near PATH_MAX limits
3. **Unicode in paths**: Non-ASCII characters in workspace names
4. **Concurrent session creation**: Multiple sessions same user same time
5. **Large config objects**: JSONB with maximum practical size
6. **Session timeout edge case**: Activity at exactly timeout boundary

### Asynchronous Operations

1. **WebSocket message ordering**: Ensure messages processed in order
2. **Cleanup race conditions**: Session closed during cleanup
3. **Concurrent path validation**: Multiple validations same path
4. **Database transaction isolation**: Concurrent reads/writes same user

---

## Test Environment Requirements

### Database
- PostgreSQL 14+ instance
- Test database with migration support
- Connection string via `DATABASE_URL`

### Environment Variables
```bash
DATABASE_URL=postgres://test:test@localhost:5432/vibe_kanban_test
JWT_SECRET=test-secret-for-jwt-signing
CONFIG_ENCRYPTION_KEY=test-encryption-key-32-bytes!!
WORKSPACE_BASE_DIR=/tmp/test-workspaces
DEPLOYMENT_MODE=kubernetes
```

### Test Fixtures
- Pre-generated JWT tokens for User A, User B, User C
- Sample configuration objects
- Sample OAuth credentials (encrypted)

---

*Document Version: 1.0*
*Created: 2025-01-21*
*Feature: k8s-multiuser*
