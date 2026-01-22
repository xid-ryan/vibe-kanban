# Design Document: K8s Multi-User Deployment

## Overview

This document describes the technical design for converting vibe-kanban from a single-user desktop application to a multi-tenant Kubernetes deployment. The design enables multiple users to access isolated workspaces with full desktop application functionality (terminal, Git, filesystem, AI agents) through a shared Kubernetes infrastructure.

### Design Goals

1. **User Isolation**: Complete separation of user data, workspaces, and processes
2. **Scalability**: Horizontal scaling with shared persistent storage (EFS)
3. **Compatibility**: Maintain existing API contracts with minimal changes
4. **Security**: JWT-based authentication with path validation and data isolation
5. **Performance**: Sub-200ms API response times with efficient database queries

### Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| PostgreSQL shared database | Enables multi-pod access, connection pooling, and transactional integrity |
| JWT middleware at route level | Allows gradual rollout and coexistence with desktop mode |
| Container-internal directory isolation | Simpler than per-user containers, leverages existing filesystem operations |
| Database-backed configuration | Enables cross-pod consistency and user-specific settings |

---

## Architecture Design

### System Architecture Diagram

```mermaid
graph TB
    subgraph External
        IdP[Identity Provider<br/>JWT Issuer]
        Users[Users]
    end

    subgraph "Kubernetes Cluster"
        subgraph Ingress
            ALB[AWS ALB<br/>TLS Termination]
        end

        subgraph "vibe-kanban Pods"
            Pod1[Pod 1]
            Pod2[Pod 2]
            PodN[Pod N...]
        end

        subgraph "Shared Services"
            PG[(PostgreSQL<br/>RDS)]
            EFS[(EFS Volume<br/>/workspaces)]
        end
    end

    Users -->|HTTPS| ALB
    IdP -.->|Issues JWT| Users
    ALB -->|Route| Pod1
    ALB -->|Route| Pod2
    ALB -->|Route| PodN

    Pod1 -->|Connection Pool| PG
    Pod2 -->|Connection Pool| PG
    PodN -->|Connection Pool| PG

    Pod1 -->|ReadWriteMany| EFS
    Pod2 -->|ReadWriteMany| EFS
    PodN -->|ReadWriteMany| EFS
```

### Pod Internal Architecture

```mermaid
graph TB
    subgraph "vibe-kanban Pod"
        subgraph "HTTP Layer"
            Router[Axum Router]
            AuthMW[Auth Middleware]
        end

        subgraph "Services Layer"
            LocalDeploy[LocalDeployment<br/>K8s Mode]
            DBSvc[DBService<br/>PostgreSQL]
            PtySvc[PtyService]
            GitSvc[GitService]
            FSSvc[FilesystemService]
            ContSvc[ContainerService]
            ConfigSvc[ConfigService]
            WsMgr[WorkspaceManager]
        end

        subgraph "Data Layer"
            Pool[(PgPool)]
            WsDir[/workspaces/<br/>user_id/]
        end
    end

    Router --> AuthMW
    AuthMW -->|UserContext| LocalDeploy

    LocalDeploy --> DBSvc
    LocalDeploy --> PtySvc
    LocalDeploy --> GitSvc
    LocalDeploy --> FSSvc
    LocalDeploy --> ContSvc
    LocalDeploy --> ConfigSvc
    LocalDeploy --> WsMgr

    DBSvc --> Pool
    ConfigSvc --> Pool
    WsMgr --> WsDir
    PtySvc --> WsDir
    GitSvc --> WsDir
    FSSvc --> WsDir
```

### Data Flow Diagram

```mermaid
sequenceDiagram
    participant User
    participant ALB
    participant AuthMW as Auth Middleware
    participant Route as Route Handler
    participant Service as Service Layer
    participant DB as PostgreSQL
    participant FS as /workspaces/{user_id}

    User->>ALB: Request + JWT
    ALB->>AuthMW: Forward Request
    AuthMW->>AuthMW: Validate JWT
    AuthMW->>AuthMW: Extract UserContext
    AuthMW->>Route: Request + UserContext
    Route->>Service: Operation(UserContext, params)
    Service->>DB: Query with user_id filter
    DB-->>Service: User's data only
    Service->>FS: Filesystem op within user dir
    FS-->>Service: Result
    Service-->>Route: Response
    Route-->>User: JSON Response
```

---

## Component Design

### Component 1: Auth Middleware

**Location**: `crates/server/src/middleware/auth.rs` (new file)

**Responsibilities**:
- Extract and validate JWT tokens from Authorization header
- Parse user claims (user_id, email) from token
- Inject UserContext into request extensions
- Reject unauthenticated/invalid requests with appropriate HTTP status

**Interfaces**:

```rust
/// User context extracted from JWT, propagated through all requests
#[derive(Clone, Debug)]
pub struct UserContext {
    pub user_id: Uuid,
    pub email: Option<String>,
}

/// Auth middleware function for protected routes
pub async fn require_auth<B>(
    State(state): State<AppState>,
    mut request: Request<B>,
    next: Next<B>,
) -> Result<Response, AuthError>;

/// Extract UserContext from request extensions
pub fn extract_user_context(request: &Request) -> Result<&UserContext, AuthError>;
```

**Dependencies**:
- `jsonwebtoken` crate for JWT validation
- `JWT_SECRET` environment variable
- Axum middleware infrastructure

---

### Component 2: DBService (PostgreSQL)

**Location**: `crates/db/src/lib.rs` (modified)

**Responsibilities**:
- Manage PostgreSQL connection pool
- Run migrations automatically on startup
- Provide typed query interface with SQLx

**Interfaces**:

```rust
pub struct DBService {
    pub pool: Pool<Postgres>,
}

impl DBService {
    /// Create new DB service with PostgreSQL connection
    pub async fn new() -> Result<DBService, Error>;

    /// Create with custom connection hook (for event triggers)
    pub async fn new_with_after_connect<F>(after_connect: F) -> Result<DBService, Error>;
}
```

**Dependencies**:
- `DATABASE_URL` environment variable
- PostgreSQL 14+
- SQLx with PostgreSQL feature

---

### Component 3: WorkspaceManager (Multi-User)

**Location**: `crates/services/src/services/workspace_manager.rs` (modified)

**Responsibilities**:
- Calculate user-specific workspace base directory
- Validate workspace paths against user boundaries
- Create/delete workspaces within user directory
- Manage worktrees for repositories

**Interfaces**:

```rust
impl WorkspaceManager {
    /// Get base directory for a specific user
    pub fn get_workspace_base_dir_for_user(user_id: &Uuid) -> PathBuf;

    /// Validate that a path is within user's workspace
    pub fn validate_user_path(user_id: &Uuid, path: &Path) -> Result<(), WorkspaceError>;

    /// Create workspace with user context
    pub async fn create_workspace(
        user_id: &Uuid,
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<WorktreeContainer, WorkspaceError>;
}
```

**Dependencies**:
- WorktreeManager
- UserContext from middleware
- `/workspaces` base mount path

---

### Component 4: ConfigService (Database-Backed)

**Location**: `crates/services/src/services/config_db.rs` (new file)

**Responsibilities**:
- Load/save user configuration from PostgreSQL
- Provide default configuration for new users
- Handle OAuth credential encryption/decryption

**Interfaces**:

```rust
pub struct ConfigService {
    pool: Pool<Postgres>,
    encryption_key: Vec<u8>,
}

impl ConfigService {
    /// Load configuration for a user
    pub async fn load_config(&self, user_id: Uuid) -> Result<Config, ConfigError>;

    /// Save configuration for a user
    pub async fn save_config(&self, user_id: Uuid, config: &Config) -> Result<(), ConfigError>;

    /// Get OAuth credentials (decrypted)
    pub async fn get_credentials(&self, user_id: Uuid) -> Result<Option<Credentials>, ConfigError>;

    /// Save OAuth credentials (encrypted)
    pub async fn save_credentials(&self, user_id: Uuid, creds: &Credentials) -> Result<(), ConfigError>;
}
```

**Dependencies**:
- PostgreSQL connection pool
- `CONFIG_ENCRYPTION_KEY` environment variable
- AES-256-GCM encryption (via `aes-gcm` crate)

---

### Component 5: PtyService (Multi-User)

**Location**: `crates/local-deployment/src/pty.rs` (modified)

**Responsibilities**:
- Create PTY sessions within user workspace directory
- Track session ownership by user_id
- Enforce session isolation between users
- Manage session lifecycle and cleanup

**Interfaces**:

```rust
pub struct PtySession {
    user_id: Uuid,
    writer: Box<dyn Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
}

impl PtyService {
    /// Create session with user ownership
    pub async fn create_session(
        &self,
        user_id: Uuid,
        working_dir: PathBuf,
        cols: u16,
        rows: u16,
    ) -> Result<(Uuid, mpsc::UnboundedReceiver<Vec<u8>>), PtyError>;

    /// Validate session ownership before operations
    pub fn validate_session_ownership(&self, session_id: &Uuid, user_id: &Uuid) -> Result<(), PtyError>;

    /// List sessions for a specific user
    pub fn list_user_sessions(&self, user_id: &Uuid) -> Vec<Uuid>;
}
```

**Dependencies**:
- portable_pty crate
- UserContext for ownership tracking

---

### Component 6: LocalDeployment (K8s Mode)

**Location**: `crates/local-deployment/src/lib.rs` (modified)

**Responsibilities**:
- Detect deployment mode (desktop vs K8s)
- Initialize services with appropriate backends
- Propagate UserContext to all service operations

**Interfaces**:

```rust
#[derive(Clone)]
pub struct LocalDeployment {
    config: Arc<RwLock<Config>>,
    db: DBService,
    // ... other services
    mode: DeploymentMode,
}

#[derive(Clone, Copy, PartialEq)]
pub enum DeploymentMode {
    Desktop,
    Kubernetes,
}

impl LocalDeployment {
    /// Create deployment based on environment
    pub async fn new() -> Result<Self, DeploymentError>;

    /// Get deployment mode
    pub fn mode(&self) -> DeploymentMode;

    /// Check if running in K8s mode
    pub fn is_k8s_mode(&self) -> bool;
}
```

---

## Data Model

### Core Data Structure Definitions

```typescript
// UserContext - extracted from JWT, used throughout the system
interface UserContext {
  user_id: string;      // UUID v4
  email: string | null; // Optional email from JWT claims
}

// Database tables with user_id column
interface Project {
  id: string;           // UUID
  user_id: string;      // UUID - owner
  name: string;
  created_at: string;   // ISO 8601
  updated_at: string;   // ISO 8601
}

interface Workspace {
  id: string;           // UUID
  user_id: string;      // UUID - owner
  task_id: string;      // UUID
  branch: string;
  container_ref: string | null;
  agent_working_dir: string | null;
  expires_at: string;   // ISO 8601
  created_at: string;   // ISO 8601
}

interface UserConfig {
  user_id: string;      // UUID - primary key
  config_json: object;  // JSON - Config struct
  oauth_credentials: string | null; // Encrypted JSON
  updated_at: string;   // ISO 8601
}

interface PtySession {
  id: string;           // UUID
  user_id: string;      // UUID - owner
  workspace_id: string | null; // UUID
  created_at: string;   // ISO 8601
  last_activity_at: string; // ISO 8601
}
```

### Data Model Diagram

```mermaid
erDiagram
    USER_CONFIG {
        uuid user_id PK
        jsonb config_json
        bytea oauth_credentials
        timestamp updated_at
    }

    PROJECTS {
        uuid id PK
        uuid user_id FK
        varchar name
        timestamp created_at
        timestamp updated_at
    }

    TASKS {
        uuid id PK
        uuid user_id FK
        uuid project_id FK
        varchar title
        text description
        varchar status
        timestamp created_at
    }

    WORKSPACES {
        uuid id PK
        uuid user_id FK
        uuid task_id FK
        varchar branch
        varchar container_ref
        varchar agent_working_dir
        timestamp expires_at
    }

    SESSIONS {
        uuid id PK
        uuid user_id FK
        uuid workspace_id FK
        varchar executor
        timestamp created_at
    }

    PTY_SESSIONS {
        uuid id PK
        uuid user_id FK
        uuid workspace_id FK
        timestamp created_at
        timestamp last_activity_at
    }

    EXECUTION_PROCESSES {
        uuid id PK
        uuid user_id FK
        uuid session_id FK
        varchar status
        bigint exit_code
        timestamp created_at
    }

    USER_CONFIG ||--o{ PROJECTS : "owns"
    USER_CONFIG ||--o{ TASKS : "owns"
    PROJECTS ||--o{ TASKS : "contains"
    TASKS ||--o{ WORKSPACES : "has"
    USER_CONFIG ||--o{ WORKSPACES : "owns"
    WORKSPACES ||--o{ SESSIONS : "has"
    SESSIONS ||--o{ EXECUTION_PROCESSES : "runs"
    USER_CONFIG ||--o{ PTY_SESSIONS : "owns"
    WORKSPACES ||--o{ PTY_SESSIONS : "associated with"
```

---

## Business Process

### Process 1: Authenticated Request Flow

```mermaid
flowchart TD
    A[HTTP Request Received] --> B{Has Authorization Header?}
    B -->|No| C[Return 401 Unauthorized]
    B -->|Yes| D[Extract Bearer Token]
    D --> E{Token Valid?}
    E -->|No| F[Return 401 Invalid Token]
    E -->|Yes| G{Has user_id claim?}
    G -->|No| H[Return 400 Bad Request]
    G -->|Yes| I[Create UserContext]
    I --> J[Attach to Request Extensions]
    J --> K[Continue to Route Handler]
    K --> L[Route extracts UserContext]
    L --> M[Service operation with user_id]
    M --> N[Database query with user_id filter]
    N --> O[Return Response]
```

### Process 2: Workspace Creation with User Isolation

```mermaid
flowchart TD
    A[POST /api/workspaces] --> B[AuthMiddleware.require_auth]
    B --> C[Extract UserContext]
    C --> D[WorkspaceHandler.create]
    D --> E[WorkspaceManager.get_workspace_base_dir_for_user]
    E --> F[Calculate: /workspaces/user_id/workspace_name]
    F --> G[Validate path within user boundary]
    G --> H{Path Valid?}
    H -->|No| I[Return 403 Forbidden]
    H -->|Yes| J[Create worktrees for repos]
    J --> K[Insert workspace record with user_id]
    K --> L[Return workspace details]
```

### Process 3: PTY Session with User Ownership

```mermaid
flowchart TD
    A[WebSocket Connect /api/terminal] --> B[AuthMiddleware.require_auth]
    B --> C[Extract UserContext]
    C --> D[PtyService.create_session]
    D --> E{Working dir within user workspace?}
    E -->|No| F[Return Error: Invalid Path]
    E -->|Yes| G[Create PTY with user ownership]
    G --> H[Set HOME to user workspace dir]
    H --> I[Store session with user_id]
    I --> J[Return session_id + output stream]

    K[WebSocket Message] --> L[PtyService.validate_session_ownership]
    L --> M{Owns session?}
    M -->|No| N[Return 404 Not Found]
    M -->|Yes| O[Process PTY command]
```

### Process 4: Configuration Load/Save

```mermaid
flowchart TD
    A[GET /api/config] --> B[AuthMiddleware.require_auth]
    B --> C[Extract UserContext]
    C --> D[ConfigService.load_config user_id]
    D --> E{Config exists?}
    E -->|No| F[Return default Config]
    E -->|Yes| G[Return stored Config]

    H[PUT /api/config] --> I[AuthMiddleware.require_auth]
    I --> J[Extract UserContext]
    J --> K[ConfigService.save_config user_id config]
    K --> L[UPSERT into user_configs]
    L --> M[Return success]
```

---

## API Design

### Modified Endpoints

All existing endpoints continue to work with the same request/response format. The only change is that they now require a valid JWT token and automatically scope operations to the authenticated user.

| Endpoint | Method | Change |
|----------|--------|--------|
| `/api/projects` | GET/POST | Filtered/scoped by user_id |
| `/api/projects/{id}` | GET/PUT/DELETE | Validates user ownership |
| `/api/tasks` | GET/POST | Filtered/scoped by user_id |
| `/api/tasks/{id}` | GET/PUT/DELETE | Validates user ownership |
| `/api/workspaces` | GET/POST | Filtered/scoped by user_id |
| `/api/workspaces/{id}` | GET/PUT/DELETE | Validates user ownership |
| `/api/config` | GET/PUT | User-specific configuration |
| `/api/terminal` | WebSocket | User session isolation |
| `/api/filesystem/*` | GET | Restricted to user workspace |

### Authentication Headers

```
Authorization: Bearer <jwt_token>
```

### Error Response Changes

```typescript
// 401 Unauthorized - Missing or invalid token
{
  "error": "unauthorized",
  "message": "Invalid or expired token"
}

// 403 Forbidden - Path outside user boundary (returned as 404 for security)
{
  "error": "not_found",
  "message": "Resource not found"
}

// 400 Bad Request - Missing required claims
{
  "error": "bad_request",
  "message": "Missing user_id claim in token"
}
```

---

## Database Design

### PostgreSQL Schema

```sql
-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- User configurations table (new)
CREATE TABLE user_configs (
    user_id UUID PRIMARY KEY,
    config_json JSONB NOT NULL DEFAULT '{}',
    oauth_credentials BYTEA,  -- Encrypted
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Add user_id to existing tables
ALTER TABLE projects ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE tasks ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE workspaces ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE sessions ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE execution_processes ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE repos ADD COLUMN user_id UUID NOT NULL;

-- Create indexes for user_id queries
CREATE INDEX idx_projects_user_id ON projects(user_id);
CREATE INDEX idx_tasks_user_id ON tasks(user_id);
CREATE INDEX idx_workspaces_user_id ON workspaces(user_id);
CREATE INDEX idx_sessions_user_id ON sessions(user_id);
CREATE INDEX idx_execution_processes_user_id ON execution_processes(user_id);
CREATE INDEX idx_repos_user_id ON repos(user_id);

-- Composite indexes for common queries
CREATE INDEX idx_tasks_user_project ON tasks(user_id, project_id);
CREATE INDEX idx_workspaces_user_task ON workspaces(user_id, task_id);
CREATE INDEX idx_sessions_user_workspace ON sessions(user_id, workspace_id);
CREATE INDEX idx_workspaces_user_expires ON workspaces(user_id, expires_at);

-- PTY sessions tracking table (new)
CREATE TABLE pty_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL,
    workspace_id UUID REFERENCES workspaces(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pty_sessions_user ON pty_sessions(user_id);
CREATE INDEX idx_pty_sessions_activity ON pty_sessions(last_activity_at);
```

### Migration Strategy

1. **Phase 1**: Create new tables and columns with nullable user_id
2. **Phase 2**: Backfill user_id for existing data (if migrating from desktop)
3. **Phase 3**: Add NOT NULL constraint
4. **Phase 4**: Create indexes

```sql
-- Migration: 001_add_user_id_columns.sql
-- Phase 1: Add nullable columns
ALTER TABLE projects ADD COLUMN IF NOT EXISTS user_id UUID;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS user_id UUID;
-- ... other tables

-- Migration: 002_user_id_not_null.sql
-- Phase 3: Add NOT NULL after data migration
ALTER TABLE projects ALTER COLUMN user_id SET NOT NULL;
ALTER TABLE tasks ALTER COLUMN user_id SET NOT NULL;
-- ... other tables

-- Migration: 003_add_user_indexes.sql
-- Phase 4: Create indexes
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_projects_user_id ON projects(user_id);
-- ... other indexes
```

---

## Infrastructure Design

### Kubernetes Manifests

#### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: vibe-kanban-desktop
  namespace: vibe
spec:
  replicas: 2
  selector:
    matchLabels:
      app: vibe-kanban-desktop
  template:
    metadata:
      labels:
        app: vibe-kanban-desktop
    spec:
      containers:
      - name: vibe-kanban
        image: ${ECR_REPO}/vibe-kanban-desktop:latest
        ports:
        - containerPort: 8081
        env:
        - name: DEPLOYMENT_MODE
          value: "kubernetes"
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: vibe-kanban-desktop-secrets
              key: database-url
        - name: JWT_SECRET
          valueFrom:
            secretKeyRef:
              name: vibe-kanban-desktop-secrets
              key: jwt-secret
        - name: CONFIG_ENCRYPTION_KEY
          valueFrom:
            secretKeyRef:
              name: vibe-kanban-desktop-secrets
              key: config-encryption-key
        - name: WORKSPACE_BASE_DIR
          value: "/workspaces"
        volumeMounts:
        - name: workspaces
          mountPath: /workspaces
        resources:
          requests:
            memory: "512Mi"
            cpu: "250m"
          limits:
            memory: "2Gi"
            cpu: "1000m"
        readinessProbe:
          httpGet:
            path: /health
            port: 8081
          initialDelaySeconds: 10
          periodSeconds: 5
        livenessProbe:
          httpGet:
            path: /health
            port: 8081
          initialDelaySeconds: 30
          periodSeconds: 10
      volumes:
      - name: workspaces
        persistentVolumeClaim:
          claimName: vibe-kanban-workspaces
```

#### PersistentVolumeClaim (EFS)

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: vibe-kanban-workspaces
  namespace: vibe
spec:
  accessModes:
    - ReadWriteMany
  storageClassName: efs-sc
  resources:
    requests:
      storage: 500Gi
```

#### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: vibe-kanban-desktop
  namespace: vibe
spec:
  selector:
    app: vibe-kanban-desktop
  ports:
  - port: 8081
    targetPort: 8081
  type: ClusterIP
```

#### Ingress

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: vibe-kanban-desktop-ingress
  namespace: vibe
  annotations:
    kubernetes.io/ingress.class: alb
    alb.ingress.kubernetes.io/scheme: internet-facing
    alb.ingress.kubernetes.io/target-type: ip
    alb.ingress.kubernetes.io/listen-ports: '[{"HTTPS":443}]'
    alb.ingress.kubernetes.io/ssl-redirect: "443"
    alb.ingress.kubernetes.io/certificate-arn: ${ACM_CERT_ARN}
    alb.ingress.kubernetes.io/healthcheck-path: /health
    # WebSocket support
    alb.ingress.kubernetes.io/load-balancer-attributes: idle_timeout.timeout_seconds=3600
spec:
  rules:
  - host: desktop.vibe-kanban.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: vibe-kanban-desktop
            port:
              number: 8081
```

#### Secrets

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: vibe-kanban-desktop-secrets
  namespace: vibe
type: Opaque
stringData:
  database-url: "postgres://user:password@postgres.vibe.svc:5432/vibe_kanban"
  jwt-secret: "${JWT_SECRET}"
  config-encryption-key: "${CONFIG_ENCRYPTION_KEY}"
```

### EFS Storage Class

```yaml
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: efs-sc
provisioner: efs.csi.aws.com
parameters:
  provisioningMode: efs-ap
  fileSystemId: ${EFS_FILE_SYSTEM_ID}
  directoryPerms: "700"
  gidRangeStart: "1000"
  gidRangeEnd: "2000"
  basePath: "/vibe-kanban"
```

---

## Security Design

### User Isolation Mechanisms

```mermaid
flowchart TD
    subgraph "Request Level"
        A[JWT Validation] --> B[UserContext Extraction]
    end

    subgraph "Database Level"
        C[Query with user_id WHERE clause]
        D[Return 404 for unauthorized access]
    end

    subgraph "Filesystem Level"
        E[Path validation against /workspaces/user_id]
        F[Canonicalize path to prevent traversal]
        G[Reject paths outside user boundary]
    end

    subgraph "Process Level"
        H[PTY session ownership tracking]
        I[Process ownership by user_id]
        J[Session timeout and cleanup]
    end

    B --> C
    B --> E
    B --> H
    C --> D
    E --> F --> G
    H --> I --> J
```

### Path Validation Pattern

```rust
/// Validate that a path is within the user's workspace
pub fn validate_user_path(user_id: &Uuid, path: &Path) -> Result<PathBuf, SecurityError> {
    let user_base = PathBuf::from(format!("/workspaces/{}", user_id));

    // Canonicalize to resolve .. and symlinks
    let canonical_path = path.canonicalize()
        .map_err(|_| SecurityError::InvalidPath)?;

    // Ensure path starts with user base
    if !canonical_path.starts_with(&user_base) {
        return Err(SecurityError::PathOutsideBoundary);
    }

    Ok(canonical_path)
}
```

### Authorization Checks

| Resource | Check |
|----------|-------|
| Project | `project.user_id == user_context.user_id` |
| Task | `task.user_id == user_context.user_id` |
| Workspace | `workspace.user_id == user_context.user_id` |
| PTY Session | `session.user_id == user_context.user_id` |
| Filesystem | Path starts with `/workspaces/{user_id}/` |
| Config | Key is `user_context.user_id` |

---

## Error Handling Strategy

### Error Types

```rust
#[derive(Debug, Error)]
pub enum MultiUserError {
    #[error("Authentication required")]
    AuthenticationRequired,

    #[error("Invalid or expired token")]
    InvalidToken,

    #[error("Missing required claim: {0}")]
    MissingClaim(String),

    #[error("Resource not found")]
    NotFound,

    #[error("Path outside user boundary")]
    PathOutsideBoundary,

    #[error("Session not owned by user")]
    SessionNotOwned,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for MultiUserError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::AuthenticationRequired => (StatusCode::UNAUTHORIZED, "Authentication required"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid or expired token"),
            Self::MissingClaim(_) => (StatusCode::BAD_REQUEST, "Invalid token claims"),
            Self::NotFound | Self::PathOutsideBoundary | Self::SessionNotOwned => {
                // Return 404 to prevent data enumeration
                (StatusCode::NOT_FOUND, "Resource not found")
            }
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
```

### Error Propagation

```mermaid
flowchart LR
    A[Service Error] --> B{Error Type}
    B -->|Auth| C[401 Unauthorized]
    B -->|Not Found| D[404 Not Found]
    B -->|Permission| D
    B -->|Validation| E[400 Bad Request]
    B -->|Internal| F[500 Internal Server Error]

    C --> G[Log: Warning]
    D --> G
    E --> G
    F --> H[Log: Error + Stack Trace]
```

### Logging and Monitoring

```rust
// Structured logging for audit
tracing::info!(
    user_id = %user_context.user_id,
    action = "workspace_create",
    workspace_id = %workspace.id,
    "User created workspace"
);

// Security event logging
tracing::warn!(
    user_id = %user_context.user_id,
    attempted_path = %path.display(),
    "Unauthorized path access attempt"
);
```

---

## Testing Strategy

### Unit Tests

| Component | Test Focus |
|-----------|------------|
| Auth Middleware | JWT validation, claim extraction, error cases |
| WorkspaceManager | Path validation, user isolation |
| ConfigService | Load/save, encryption, defaults |
| PtyService | Session ownership, isolation |

### Integration Tests

```rust
#[tokio::test]
async fn test_user_isolation() {
    let app = create_test_app().await;

    // Create project as user A
    let user_a_token = create_test_jwt("user-a");
    let project = app.post("/api/projects")
        .header("Authorization", format!("Bearer {}", user_a_token))
        .json(&json!({ "name": "User A Project" }))
        .await;

    // User B should not see user A's project
    let user_b_token = create_test_jwt("user-b");
    let projects = app.get("/api/projects")
        .header("Authorization", format!("Bearer {}", user_b_token))
        .await;

    assert!(projects.json::<Vec<Project>>().is_empty());

    // User B should get 404 when accessing user A's project
    let response = app.get(&format!("/api/projects/{}", project.id))
        .header("Authorization", format!("Bearer {}", user_b_token))
        .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

### Load Tests

- Simulate 100 concurrent users
- Verify response times < 200ms at p95
- Test WebSocket connection handling under load
- Verify database connection pool behavior

---

## Migration Strategy

### Phase 1: Infrastructure Preparation

1. Provision PostgreSQL (RDS)
2. Create EFS volume with storage class
3. Deploy K8s manifests (without traffic)
4. Verify database connectivity

### Phase 2: Code Changes

1. Implement auth middleware
2. Modify DBService for PostgreSQL
3. Add user_id to all services
4. Implement path validation
5. Migrate ConfigService to database

### Phase 3: Testing

1. Deploy to staging environment
2. Run integration tests
3. Perform load testing
4. Security audit

### Phase 4: Production Rollout

1. Deploy pods with feature flag disabled
2. Enable for subset of users
3. Monitor error rates and performance
4. Gradual rollout to all users

### Backward Compatibility

- Desktop mode continues to work (detected via `DEPLOYMENT_MODE` env)
- API contracts remain unchanged
- Frontend requires no modifications (auth token handling)

---

## Appendix

### Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `DEPLOYMENT_MODE` | `desktop` or `kubernetes` | No (default: desktop) |
| `DATABASE_URL` | PostgreSQL connection string | Yes (K8s mode) |
| `JWT_SECRET` | Secret for JWT validation | Yes (K8s mode) |
| `CONFIG_ENCRYPTION_KEY` | AES-256 key for credential encryption | Yes (K8s mode) |
| `WORKSPACE_BASE_DIR` | Base directory for workspaces | No (default: /workspaces) |

### Related Documents

- Requirements: `.claude/specs/k8s-multiuser/requirements.md`
- Analysis: `docs/k8s-multiuser-deployment-analysis.md`
- Existing K8s: `k8s/vibe-kanban.yaml`
- Remote Auth Reference: `crates/remote/src/db/auth.rs`

---

*Document Version: 1.0*
*Created: 2025-01-21*
*Feature: k8s-multiuser*
