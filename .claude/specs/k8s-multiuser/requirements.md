# Requirements Document: K8s Multi-User Deployment

## Introduction

This document defines the requirements for converting the vibe-kanban desktop application into a multi-user Kubernetes deployment. The goal is to provide the full desktop application experience (terminal, Git, filesystem, AI agents) in a shared Kubernetes environment while ensuring proper user isolation, security, and scalability.

Currently, vibe-kanban operates as a single-user desktop application with local SQLite storage and file-based configuration. This feature transforms it into a multi-tenant cloud service where multiple users can access their own isolated workspaces through a shared Kubernetes infrastructure.

### Key Objectives
- Deliver complete desktop app functionality (terminal, Git, filesystem, AI agents) on Kubernetes
- Ensure user isolation via container-internal directory structure
- Maintain terminal functionality as a local shell within the container
- Support horizontal scaling with shared persistent storage

---

## Requirements

### Requirement 1: User Authentication Middleware

**User Story:** As a platform administrator, I want all API requests to be authenticated with JWT tokens so that each user's identity can be verified and their data isolated.

#### Acceptance Criteria

1. WHEN an HTTP request is received without an Authorization header THEN the system SHALL return a 401 Unauthorized response.

2. WHEN an HTTP request contains an invalid or expired JWT token THEN the system SHALL return a 401 Unauthorized response with an appropriate error message.

3. WHEN an HTTP request contains a valid JWT token THEN the system SHALL extract the user_id and email from the token claims and attach them to a UserContext object.

4. WHEN a UserContext is created THEN the system SHALL propagate it to all downstream services via request extensions.

5. WHERE the authentication middleware is applied THEN the system SHALL validate tokens against a configurable JWT secret from environment variables.

6. IF a token is valid but the user_id claim is missing THEN the system SHALL reject the request with a 400 Bad Request response.

---

### Requirement 2: Database Migration (SQLite to PostgreSQL)

**User Story:** As a developer, I want the application to use PostgreSQL instead of SQLite so that multiple pods can share the same database and user data can be properly isolated.

#### Acceptance Criteria

1. WHEN the application starts THEN the system SHALL connect to PostgreSQL using the DATABASE_URL environment variable.

2. WHEN connecting to the database THEN the system SHALL use a connection pool with configurable maximum connections (default: 10).

3. WHEN the application starts THEN the system SHALL run database migrations automatically to ensure schema is up to date.

4. WHERE database tables store user-specific data (projects, tasks, workspaces, sessions, configs) THEN the system SHALL include a user_id column with a NOT NULL constraint.

5. WHEN querying user-specific data THEN the system SHALL always filter by the authenticated user's user_id.

6. WHEN inserting user-specific data THEN the system SHALL automatically set the user_id from the UserContext.

7. WHERE indexes exist on frequently queried columns THEN the system SHALL include composite indexes on (user_id, other_columns) for performance.

8. IF a user attempts to access another user's data THEN the system SHALL return a 404 Not Found response (not 403, to prevent data enumeration).

---

### Requirement 3: Workspace Isolation

**User Story:** As a user, I want my workspaces and files to be completely isolated from other users so that my code and data remain private and secure.

#### Acceptance Criteria

1. WHEN a user creates a workspace THEN the system SHALL create it under the path `/workspaces/{user_id}/{workspace_name}`.

2. WHEN the WorkspaceManager calculates base directory THEN the system SHALL use the pattern `/workspaces/{user_id}/` where user_id is from UserContext.

3. WHEN a workspace path is requested THEN the system SHALL validate that the path starts with the user's base directory.

4. IF a workspace path does not start with the user's base directory THEN the system SHALL return an Unauthorized error.

5. WHEN listing workspaces THEN the system SHALL only return workspaces owned by the authenticated user.

6. WHEN the application initializes THEN the system SHALL create the user's base workspace directory if it does not exist.

7. WHERE filesystem operations occur THEN the system SHALL enforce that all paths are within the user's workspace directory.

---

### Requirement 4: Configuration Storage Migration

**User Story:** As a user, I want my application settings to persist across sessions and be accessible from any pod so that my preferences are always available.

#### Acceptance Criteria

1. WHEN loading user configuration THEN the system SHALL query the database by user_id instead of reading from a local file.

2. WHEN saving user configuration THEN the system SHALL upsert to the database with the user's user_id.

3. IF no configuration exists for a user THEN the system SHALL return sensible default configuration values.

4. WHEN configuration is saved THEN the system SHALL update the `updated_at` timestamp automatically.

5. WHERE OAuth credentials are stored THEN the system SHALL store them encrypted in the database associated with the user_id.

6. WHEN retrieving OAuth credentials THEN the system SHALL decrypt and return credentials only for the authenticated user.

---

### Requirement 5: PTY/Terminal Service Multi-User Support

**User Story:** As a user, I want to use terminal sessions that operate within my workspace so that I can run commands in my isolated environment.

#### Acceptance Criteria

1. WHEN creating a PTY session THEN the system SHALL set the working directory to a path within the user's workspace.

2. WHEN a PTY session starts THEN the system SHALL set environment variables including HOME to point to the user's workspace directory.

3. WHERE PTY sessions are stored THEN the system SHALL associate each session with a user_id.

4. WHEN listing active PTY sessions THEN the system SHALL only return sessions owned by the authenticated user.

5. WHEN a PTY session is idle for a configurable timeout period (default: 30 minutes) THEN the system SHALL automatically terminate the session.

6. IF a user attempts to access another user's PTY session THEN the system SHALL return a 404 Not Found error.

7. WHILE a PTY session is active THEN the system SHALL track memory usage and enforce configurable limits per user.

---

### Requirement 6: Git Service Multi-User Support

**User Story:** As a user, I want Git operations to work within my workspace so that I can manage version control for my projects.

#### Acceptance Criteria

1. WHEN performing Git operations THEN the system SHALL validate that the repository path is within the user's workspace directory.

2. WHEN creating Git worktrees THEN the system SHALL place them within the user's workspace base directory.

3. IF a Git operation targets a path outside the user's workspace THEN the system SHALL return an Unauthorized error.

4. WHEN Git credentials are required THEN the system SHALL use the user's stored OAuth credentials from the database.

5. WHERE Git operations may conflict with concurrent operations THEN the system SHALL implement file-level locking to prevent corruption.

---

### Requirement 7: Container/Process Service Multi-User Support

**User Story:** As a user, I want to run AI agents (Claude Code, Codex, etc.) that operate within my workspace so that automated coding assistance works correctly.

#### Acceptance Criteria

1. WHEN spawning a process THEN the system SHALL set the working directory to a path within the user's workspace.

2. WHEN tracking child processes THEN the system SHALL associate each process with the user_id who initiated it.

3. WHEN listing active processes THEN the system SHALL only return processes owned by the authenticated user.

4. IF a user attempts to terminate another user's process THEN the system SHALL return a 404 Not Found error.

5. WHERE process output is stored THEN the system SHALL isolate message stores by user_id.

6. WHEN a process completes or is terminated THEN the system SHALL clean up associated resources (message stores, interrupt senders).

---

### Requirement 8: Filesystem Service Multi-User Support

**User Story:** As a user, I want to browse and manage files within my workspace so that I can organize my project files.

#### Acceptance Criteria

1. WHEN listing directories THEN the system SHALL restrict results to paths within the user's workspace.

2. WHEN the default path is requested THEN the system SHALL return the user's workspace base directory instead of the system home directory.

3. IF a path traversal attempt is detected (e.g., using `..`) THEN the system SHALL validate the resolved path is still within the user's workspace.

4. WHEN searching for Git repositories THEN the system SHALL only search within the user's workspace directory.

5. WHERE file operations support filtering THEN the system SHALL apply user workspace path constraints before any other filters.

---

### Requirement 9: Kubernetes Deployment Infrastructure

**User Story:** As a platform operator, I want the application deployed on Kubernetes with proper resource management so that it can scale to support multiple users.

#### Acceptance Criteria

1. WHEN the Deployment is created THEN the system SHALL support multiple replicas (minimum 2 for high availability).

2. WHERE workspaces are stored THEN the system SHALL mount a PersistentVolumeClaim with ReadWriteMany access mode.

3. WHEN using AWS THEN the system SHALL configure EFS (Elastic File System) as the storage class for multi-pod access.

4. WHERE secrets are required (DATABASE_URL, JWT_SECRET) THEN the system SHALL reference them from Kubernetes Secrets.

5. WHEN the Ingress is configured THEN the system SHALL terminate TLS at the ALB with a valid ACM certificate.

6. WHERE health checks are configured THEN the system SHALL include both liveness and readiness probes.

7. WHEN resource limits are specified THEN the system SHALL configure appropriate CPU and memory limits per pod.

---

### Requirement 10: WebSocket Connection Handling

**User Story:** As a user, I want terminal and real-time features to work reliably so that my coding experience is not interrupted by network issues.

#### Acceptance Criteria

1. WHEN a WebSocket connection is established for PTY THEN the system SHALL associate it with the user's session.

2. IF a WebSocket connection is lost THEN the system SHALL maintain the PTY session state for a configurable grace period (default: 5 minutes).

3. WHEN a user reconnects to a PTY session within the grace period THEN the system SHALL restore the session state.

4. WHERE the Ingress handles WebSocket connections THEN the system SHALL configure appropriate timeouts for long-lived connections.

5. WHEN WebSocket messages are received THEN the system SHALL validate that the user owns the target session.

---

### Requirement 11: Session and Resource Cleanup

**User Story:** As a platform operator, I want automatic cleanup of stale resources so that the system remains performant and storage is managed efficiently.

#### Acceptance Criteria

1. WHEN a PTY session has been idle beyond the timeout THEN the system SHALL terminate and clean up the session.

2. WHEN a workspace has not been accessed for a configurable period (default: 30 days) THEN the system SHALL mark it for cleanup review.

3. WHERE orphaned processes exist (no active session or user connection) THEN the system SHALL terminate them during periodic cleanup.

4. WHEN cleanup runs THEN the system SHALL log all cleanup actions for audit purposes.

5. IF a user's storage quota is exceeded THEN the system SHALL prevent new workspace creation until space is freed.

---

### Requirement 12: Security and Authorization

**User Story:** As a security administrator, I want comprehensive access controls so that users cannot access or modify other users' data.

#### Acceptance Criteria

1. WHERE sensitive data is transmitted THEN the system SHALL require TLS encryption (HTTPS/WSS).

2. WHEN storing OAuth tokens THEN the system SHALL encrypt them at rest using a configurable encryption key.

3. WHERE user actions occur THEN the system SHALL log the user_id, action type, and timestamp for audit.

4. IF multiple failed authentication attempts occur from an IP THEN the system SHALL implement rate limiting.

5. WHEN JWT tokens are issued THEN the system SHALL include an expiration time (default: 24 hours).

6. WHERE file operations occur THEN the system SHALL run with minimal filesystem permissions (principle of least privilege).

7. IF an unauthorized access attempt is detected THEN the system SHALL log the attempt with full context for security review.

---

## Non-Functional Requirements

### Performance Requirements

1. The system SHALL support at least 100 concurrent users per pod.
2. API response time SHALL be less than 200ms for 95th percentile requests (excluding long-running operations).
3. PTY input latency SHALL be less than 50ms under normal load.
4. Database queries SHALL complete within 100ms for 99th percentile.

### Scalability Requirements

1. The system SHALL support horizontal scaling by adding more pods.
2. Storage SHALL support at least 100GB per user with ability to expand.
3. The system SHALL handle at least 1000 active workspaces across all users.

### Availability Requirements

1. The system SHALL maintain 99.9% uptime for API endpoints.
2. Pod failures SHALL not result in data loss.
3. Database connections SHALL be pooled and resilient to transient failures.

### Maintainability Requirements

1. All configuration SHALL be externalized via environment variables or ConfigMaps.
2. Database migrations SHALL be idempotent and reversible.
3. Logs SHALL be structured (JSON) for easy parsing and aggregation.

---

## Scope

### In Scope

- User authentication via JWT tokens
- Database migration from SQLite to PostgreSQL
- User workspace isolation at filesystem level
- Configuration storage in database
- Multi-user PTY session management
- Multi-user Git operations
- Multi-user process/agent execution
- Kubernetes deployment manifests
- Ingress configuration with TLS
- Persistent volume configuration for workspaces
- WebSocket session management
- Resource cleanup automation

### Out of Scope

- User registration and account management (assumed to be handled by external identity provider)
- JWT token issuance (assumed to be handled by external auth service)
- Billing and quota enforcement beyond basic storage limits
- Custom domain support per user
- Multi-region deployment
- Database backup and disaster recovery procedures
- CI/CD pipeline modifications
- Frontend modifications (assumed to work with existing API contracts)
- Migration of existing desktop user data

---

## Assumptions

1. An external identity provider (IdP) handles user authentication and issues JWT tokens.
2. The JWT tokens contain `sub` (user_id as UUID) and optionally `email` claims.
3. AWS EKS is the target Kubernetes platform with EFS available for storage.
4. PostgreSQL database is provisioned separately (e.g., AWS RDS).
5. Users have valid OAuth credentials for Git operations with external services.
6. The frontend application will be updated separately to handle authentication flows.
7. Existing API contracts will remain largely unchanged (only adding user context).

---

## Dependencies

### External Services
- PostgreSQL database (version 14+)
- AWS EFS for persistent storage
- AWS ALB for ingress
- AWS ACM for TLS certificates
- External Identity Provider for JWT tokens

### Internal Dependencies
- Existing crates/local-deployment infrastructure
- Existing API route structure in crates/server
- Existing service interfaces (DBService, GitService, PtyService, etc.)

### Technology Stack
- Rust with SQLx for PostgreSQL
- Axum web framework with middleware support
- Kubernetes 1.24+
- Docker for container images

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| PTY session memory leaks | High | Medium | Implement session timeouts and automatic cleanup |
| Filesystem permission conflicts | Medium | Medium | Use consistent UID/GID across pods, implement proper path validation |
| WebSocket connection drops | Medium | High | Implement reconnection logic and session state preservation |
| Git operation conflicts | Low | Low | Implement file-level locking and retry mechanisms |
| Storage exhaustion | High | Low | Implement quotas and monitoring alerts |
| Database connection pool exhaustion | High | Medium | Configure appropriate pool sizes and connection timeouts |

---

## Glossary

- **UserContext**: A struct containing user_id and email extracted from JWT, propagated through all services
- **Workspace**: A directory structure containing worktrees for a user's projects
- **Worktree**: A Git worktree representing a specific branch of a repository
- **PTY**: Pseudo-terminal, enabling interactive shell sessions
- **EFS**: AWS Elastic File System, providing shared storage with ReadWriteMany access

---

*Document Version: 1.0*
*Created: 2025-01-21*
*Feature: k8s-multiuser*
