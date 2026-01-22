# K8s 멀티유저 배포를 위한 코드 분석 및 수정 계획

## 개요

이 문서는 vibe-kanban 데스크톱 앱을 Kubernetes에 멀티유저 환경으로 배포하기 위한 코드 분석 결과와 수정 계획을 정리합니다.

### 목표

- 데스크톱 앱의 전체 기능(터미널, Git, 파일 시스템, AI 에이전트)을 K8s에서 제공
- 사용자별 격리 (컨테이너 내 디렉토리 기반)
- 터미널은 컨테이너의 로컬 쉘로 동작

---

## 1. 현재 아키텍처 분석

### 1.1 전체 구조

```
┌─────────────────────────────────────────────────────────────┐
│                    LocalDeployment                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐           │
│  │  DBService  │ │ GitService  │ │  PtyService │           │
│  │  (SQLite)   │ │ (libgit2)   │ │ (portable_  │           │
│  │             │ │             │ │    pty)     │           │
│  └─────────────┘ └─────────────┘ └─────────────┘           │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐           │
│  │ Filesystem  │ │ Container   │ │  AuthContext│           │
│  │  Service    │ │   Service   │ │ (File-based)│           │
│  └─────────────┘ └─────────────┘ └─────────────┘           │
└─────────────────────────────────────────────────────────────┘
                           │
                    ~/vibe-kanban/
                           │
              ┌────────────┴────────────┐
              │                         │
         db.sqlite              worktrees/
         config.json          {workspace}/
```

### 1.2 핵심 컴포넌트

| 컴포넌트 | 파일 경로 | 역할 |
|---------|----------|------|
| LocalDeployment | `crates/local-deployment/src/lib.rs` | 모든 서비스의 컨테이너, 앱 진입점 |
| DBService | `crates/db/src/lib.rs` | SQLite 데이터베이스 관리 |
| PtyService | `crates/local-deployment/src/pty.rs` | 터미널 세션 관리 (portable_pty) |
| GitService | `crates/services/src/services/git.rs` | Git 작업 (libgit2 + CLI) |
| ContainerService | `crates/local-deployment/src/container.rs` | 프로세스 실행 및 워크스페이스 관리 |
| WorkspaceManager | `crates/services/src/services/workspace_manager.rs` | 워크트리 생성/삭제 |
| FilesystemService | `crates/services/src/services/filesystem.rs` | 파일/디렉토리 탐색 |
| AuthContext | `crates/services/src/services/auth.rs` | OAuth 자격증명 관리 |

---

## 2. 컴포넌트별 상세 분석

### 2.1 데이터베이스 (DBService)

**파일:** `crates/db/src/lib.rs`

**현재 구현:**
```rust
pub struct DBService {
    pub pool: Pool<Sqlite>,
}

impl DBService {
    pub async fn new() -> Result<DBService, Error> {
        let database_url = format!(
            "sqlite://{}",
            asset_dir().join("db.sqlite").to_string_lossy()
        );
        // ...
    }
}
```

**특징:**
- 단일 SQLite 파일 사용 (`~/.vibe-kanban/db.sqlite`)
- 모든 프로젝트, 태스크, 세션 데이터 저장
- 사용자 구분 없이 전역 데이터

**K8s 수정 필요도:** 🔴 높음

---

### 2.2 터미널/PTY (PtyService)

**파일:** `crates/local-deployment/src/pty.rs`

**현재 구현:**
```rust
pub async fn create_session(
    &self,
    working_dir: PathBuf,
    cols: u16,
    rows: u16,
) -> Result<(Uuid, mpsc::UnboundedReceiver<Vec<u8>>), PtyError> {
    let shell = get_interactive_shell().await;
    let mut cmd = CommandBuilder::new(&shell);
    cmd.cwd(&working_dir);
    // portable_pty로 쉘 세션 생성
}
```

**특징:**
- `portable_pty` 크레이트 사용
- `working_dir` 파라미터로 작업 디렉토리 지정
- WebSocket을 통해 프론트엔드와 통신

**K8s 수정 필요도:** 🟢 낮음 (이미 경로 기반으로 동작)

---

### 2.3 Git 서비스 (GitService)

**파일:** `crates/services/src/services/git.rs`

**현재 구현:**
```rust
pub struct GitService {}

impl GitService {
    pub fn open_repo(&self, repo_path: &Path) -> Result<Repository, GitServiceError> {
        Repository::open(repo_path)
    }

    pub fn commit(&self, path: &Path, message: &str) -> Result<bool, GitServiceError> {
        // Git CLI 사용
    }

    pub fn add_worktree(
        &self,
        repo_path: &Path,
        worktree_path: &Path,
        branch: &str,
        create_branch: bool,
    ) -> Result<(), GitServiceError> {
        // worktree 생성
    }
}
```

**특징:**
- libgit2 + Git CLI 혼합 사용
- 모든 작업이 경로(Path) 기반
- 워크트리 관리로 브랜치별 격리

**K8s 수정 필요도:** 🟢 낮음 (경로만 올바르게 전달하면 됨)

---

### 2.4 워크스페이스 관리 (WorkspaceManager)

**파일:** `crates/services/src/services/workspace_manager.rs`

**현재 구현:**
```rust
pub struct WorkspaceManager;

impl WorkspaceManager {
    pub fn get_workspace_base_dir() -> PathBuf {
        WorktreeManager::get_worktree_base_dir()
        // 기본값: ~/vibe-kanban-worktrees/
    }

    pub async fn create_workspace(
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<WorktreeContainer, WorkspaceError> {
        // 워크스페이스 디렉토리 생성
        // 각 레포지토리의 워크트리 생성
    }
}
```

**특징:**
- 전역 기본 디렉토리 사용
- 사용자 구분 없음
- 워크스페이스별 고유 디렉토리명 생성

**K8s 수정 필요도:** 🔴 높음 (사용자별 격리 필요)

---

### 2.5 컨테이너 서비스 (LocalContainerService)

**파일:** `crates/local-deployment/src/container.rs`

**현재 구현:**
```rust
pub struct LocalContainerService {
    db: DBService,
    child_store: Arc<RwLock<HashMap<Uuid, Arc<RwLock<AsyncGroupChild>>>>>,
    interrupt_senders: Arc<RwLock<HashMap<Uuid, InterruptSender>>>,
    msg_stores: Arc<RwLock<HashMap<Uuid, Arc<MsgStore>>>>,
    config: Arc<RwLock<Config>>,
    git: GitService,
    // ...
}

impl ContainerService for LocalContainerService {
    fn workspace_to_current_dir(&self, workspace: &Workspace) -> PathBuf {
        PathBuf::from(workspace.container_ref.clone().unwrap_or_default())
    }

    async fn create(&self, workspace: &Workspace) -> Result<ContainerRef, ContainerError> {
        // 워크스페이스 디렉토리 생성
        // 워크트리 생성
        // 프로젝트 파일 복사
    }
}
```

**특징:**
- AI 에이전트 실행 관리 (Claude Code, Codex 등)
- 프로세스 라이프사이클 관리
- Git 커밋 자동화

**K8s 수정 필요도:** 🟡 중간 (사용자 컨텍스트 전파 필요)

---

### 2.6 인증 컨텍스트 (AuthContext)

**파일:** `crates/services/src/services/auth.rs`

**현재 구현:**
```rust
pub struct AuthContext {
    oauth: Arc<OAuthCredentials>,
    profile: Arc<RwLock<Option<ProfileResponse>>>,
    refresh_lock: Arc<TokioMutex<()>>,
}

impl AuthContext {
    pub async fn get_credentials(&self) -> Option<Credentials> {
        self.oauth.get().await  // 파일에서 읽기
    }

    pub async fn save_credentials(&self, creds: &Credentials) -> std::io::Result<()> {
        self.oauth.save(creds).await  // 파일에 저장
    }
}
```

**특징:**
- 파일 기반 자격증명 저장 (`~/.vibe-kanban/credentials.json`)
- 단일 사용자 가정
- 메모리 내 프로필 캐싱

**K8s 수정 필요도:** 🔴 높음 (DB 기반으로 변경 필요)

---

### 2.7 파일시스템 서비스 (FilesystemService)

**파일:** `crates/services/src/services/filesystem.rs`

**현재 구현:**
```rust
pub struct FilesystemService {}

impl FilesystemService {
    pub async fn list_git_repos(
        &self,
        path: Option<String>,
        timeout_ms: u64,
        hard_timeout_ms: u64,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        let base_path = path
            .map(PathBuf::from)
            .unwrap_or_else(Self::get_home_directory);
        // 디렉토리 탐색
    }
}
```

**특징:**
- 호스트 파일시스템 직접 접근
- 홈 디렉토리 기본값 사용
- Git 레포지토리 검색 기능

**K8s 수정 필요도:** 🟡 중간 (기본 경로 변경 필요)

---

## 3. 수정 범위

### 3.1 우선순위별 분류

#### 🔴 높음 (필수)

| 영역 | 설명 | 파일 |
|-----|------|-----|
| **사용자 인증 미들웨어** | JWT/세션에서 user_id 추출 | 신규: `crates/server/src/middleware/auth.rs` |
| **DB 마이그레이션** | SQLite → PostgreSQL 또는 사용자별 SQLite | `crates/db/src/lib.rs` |
| **워크스페이스 격리** | 사용자별 디렉토리 분리 | `crates/services/src/services/workspace_manager.rs` |
| **설정 저장소** | 파일 → DB 기반 | `crates/services/src/services/config.rs` |

#### 🟡 중간

| 영역 | 설명 | 파일 |
|-----|------|-----|
| **사용자 컨텍스트 전파** | 모든 서비스에 user_id 전달 | 다수의 라우트 핸들러 |
| **파일시스템 경로** | 기본 경로 변경 | `crates/services/src/services/filesystem.rs` |
| **K8s 매니페스트** | Deployment, PVC, ConfigMap | 신규: `k8s/desktop/` |

#### 🟢 낮음

| 영역 | 설명 | 파일 |
|-----|------|-----|
| **로컬 전용 기능 제거** | 브라우저 자동 열기 등 | `crates/server/src/main.rs` |
| **PTY 서비스** | 변경 불필요 | - |
| **Git 서비스** | 변경 불필요 | - |

---

### 3.2 상세 수정 계획

#### 3.2.1 사용자 인증 미들웨어

**신규 파일:** `crates/server/src/middleware/auth.rs`

```rust
use axum::{
    extract::State,
    http::Request,
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct UserContext {
    pub user_id: Uuid,
    pub email: Option<String>,
}

pub async fn require_user<B>(
    State(state): State<AppState>,
    mut request: Request<B>,
    next: Next<B>,
) -> Result<Response, AuthError> {
    // 1. Authorization 헤더에서 JWT 추출
    let token = extract_bearer_token(&request)?;

    // 2. JWT 검증 및 user_id 추출
    let claims = verify_jwt(&token, &state.jwt_secret)?;

    // 3. UserContext를 request extensions에 추가
    request.extensions_mut().insert(UserContext {
        user_id: claims.sub,
        email: claims.email,
    });

    Ok(next.run(request).await)
}
```

**라우트 적용:**

```rust
// crates/server/src/routes/mod.rs
pub fn router(deployment: DeploymentImpl) -> IntoMakeService<Router> {
    let protected_routes = Router::new()
        .merge(projects::router(&deployment))
        .merge(tasks::router(&deployment))
        // ... 기타 라우트
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_user,
        ));

    // ...
}
```

---

#### 3.2.2 데이터베이스 마이그레이션

**방안 A: PostgreSQL 공유 (권장)**

```rust
// crates/db/src/lib.rs
pub struct DBService {
    pub pool: Pool<Postgres>,  // SQLite → Postgres
}

impl DBService {
    pub async fn new(database_url: &str) -> Result<DBService, Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(DBService { pool })
    }
}
```

**테이블 스키마 변경:**

```sql
-- 모든 테이블에 user_id 추가
ALTER TABLE projects ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE tasks ADD COLUMN user_id UUID NOT NULL;
ALTER TABLE workspaces ADD COLUMN user_id UUID NOT NULL;
-- ...

-- 인덱스 추가
CREATE INDEX idx_projects_user_id ON projects(user_id);
CREATE INDEX idx_tasks_user_id ON tasks(user_id);
```

**방안 B: 사용자별 SQLite**

```rust
impl DBService {
    pub async fn new_for_user(user_id: Uuid) -> Result<DBService, Error> {
        let db_path = format!("/data/users/{}/db.sqlite", user_id);
        std::fs::create_dir_all(format!("/data/users/{}", user_id))?;

        let database_url = format!("sqlite://{}?mode=rwc", db_path);
        // ...
    }
}
```

---

#### 3.2.3 워크스페이스 격리

**수정 파일:** `crates/services/src/services/workspace_manager.rs`

```rust
impl WorkspaceManager {
    /// 사용자별 워크스페이스 기본 디렉토리
    pub fn get_workspace_base_dir(user_id: &Uuid) -> PathBuf {
        PathBuf::from(format!("/workspaces/{}", user_id))
    }

    pub async fn create_workspace(
        user_id: &Uuid,  // 추가
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<WorktreeContainer, WorkspaceError> {
        // 사용자 디렉토리 검증
        let base_dir = Self::get_workspace_base_dir(user_id);
        if !workspace_dir.starts_with(&base_dir) {
            return Err(WorkspaceError::Unauthorized);
        }

        // 기존 로직...
    }
}
```

**수정 파일:** `crates/services/src/services/worktree_manager.rs`

```rust
impl WorktreeManager {
    pub fn get_worktree_base_dir(user_id: &Uuid) -> PathBuf {
        PathBuf::from(format!("/workspaces/{}", user_id))
    }
}
```

---

#### 3.2.4 설정 저장소

**수정 파일:** `crates/services/src/services/config.rs`

```rust
pub struct ConfigService {
    pool: Pool<Postgres>,
}

impl ConfigService {
    pub async fn load_config(&self, user_id: Uuid) -> Result<Config, Error> {
        let row = sqlx::query_as!(
            ConfigRow,
            "SELECT * FROM user_configs WHERE user_id = $1",
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Config::from(r)),
            None => Ok(Config::default()),
        }
    }

    pub async fn save_config(&self, user_id: Uuid, config: &Config) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO user_configs (user_id, config_json, updated_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (user_id) DO UPDATE SET
                config_json = EXCLUDED.config_json,
                updated_at = NOW()
            "#,
            user_id,
            serde_json::to_value(config)?
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
```

---

## 4. K8s 배포 아키텍처

### 4.1 목표 아키텍처

```
┌──────────────────────────────────────────────────────────────┐
│                        Kubernetes                            │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │                    Ingress (ALB)                       │  │
│  │                vibe-kanban.example.com                 │  │
│  └────────────────────────────────────────────────────────┘  │
│                           │                                  │
│                           ▼                                  │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              vibe-kanban-desktop (Pod)                 │  │
│  │  ┌──────────────────────────────────────────────────┐  │  │
│  │  │  Auth Middleware (JWT → UserContext)             │  │  │
│  │  └──────────────────────────────────────────────────┘  │  │
│  │  ┌──────────────────────────────────────────────────┐  │  │
│  │  │         LocalDeployment (Modified)               │  │  │
│  │  │  ┌────────────┐ ┌────────────┐ ┌────────────┐   │  │  │
│  │  │  │ DBService  │ │ PtyService │ │ GitService │   │  │  │
│  │  │  │ (Postgres) │ │            │ │            │   │  │  │
│  │  │  └────────────┘ └────────────┘ └────────────┘   │  │  │
│  │  └──────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────┘  │
│                           │                                  │
│              ┌────────────┴────────────┐                     │
│              ▼                         ▼                     │
│      ┌────────────┐           ┌────────────────┐             │
│      │ PostgreSQL │           │   PVC          │             │
│      │  (shared)  │           │ /workspaces/   │             │
│      │            │           │   ├── user1/   │             │
│      │            │           │   ├── user2/   │             │
│      │            │           │   └── user3/   │             │
│      └────────────┘           └────────────────┘             │
└──────────────────────────────────────────────────────────────┘
```

### 4.2 K8s 매니페스트

**Deployment:**

```yaml
# k8s/desktop/deployment.yaml
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
        - containerPort: 5173
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: vibe-kanban-secrets
              key: database-url
        - name: JWT_SECRET
          valueFrom:
            secretKeyRef:
              name: vibe-kanban-secrets
              key: jwt-secret
        - name: WORKSPACE_BASE_DIR
          value: "/workspaces"
        volumeMounts:
        - name: workspaces
          mountPath: /workspaces
      volumes:
      - name: workspaces
        persistentVolumeClaim:
          claimName: vibe-kanban-workspaces
```

**PersistentVolumeClaim:**

```yaml
# k8s/desktop/pvc.yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: vibe-kanban-workspaces
  namespace: vibe
spec:
  accessModes:
    - ReadWriteMany  # 여러 Pod에서 접근 가능
  storageClassName: efs-sc  # AWS EFS 사용
  resources:
    requests:
      storage: 100Gi
```

**Service:**

```yaml
# k8s/desktop/service.yaml
apiVersion: v1
kind: Service
metadata:
  name: vibe-kanban-desktop
  namespace: vibe
spec:
  selector:
    app: vibe-kanban-desktop
  ports:
  - port: 80
    targetPort: 5173
```

**Ingress:**

```yaml
# k8s/desktop/ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: vibe-kanban-desktop
  namespace: vibe
  annotations:
    kubernetes.io/ingress.class: alb
    alb.ingress.kubernetes.io/scheme: internet-facing
    alb.ingress.kubernetes.io/certificate-arn: ${ACM_CERT_ARN}
    alb.ingress.kubernetes.io/listen-ports: '[{"HTTPS":443}]'
spec:
  rules:
  - host: vibe-kanban.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: vibe-kanban-desktop
            port:
              number: 80
```

---

## 5. 구현 단계

### Phase 1: 기반 구축 (1-2주)

1. **인증 미들웨어 구현**
   - JWT 검증 로직
   - UserContext 추출 및 전파
   - 테스트 작성

2. **DB 스키마 설계**
   - PostgreSQL 마이그레이션 파일 작성
   - 기존 SQLite 스키마 변환
   - user_id 컬럼 추가

### Phase 2: 서비스 수정 (2-3주)

3. **DBService 수정**
   - PostgreSQL 연결 지원
   - 모든 쿼리에 user_id 조건 추가

4. **WorkspaceManager 수정**
   - 사용자별 디렉토리 격리
   - 경로 검증 로직 추가

5. **ConfigService 수정**
   - 파일 기반 → DB 기반
   - 사용자별 설정 저장

### Phase 3: 통합 및 배포 (1-2주)

6. **K8s 매니페스트 작성**
   - Deployment, Service, Ingress
   - PVC (EFS) 설정
   - Secret 관리

7. **Docker 이미지 빌드**
   - 멀티스테이지 빌드
   - 환경 변수 설정

8. **테스트 및 배포**
   - 통합 테스트
   - 스테이징 배포
   - 프로덕션 배포

---

## 6. 예상 작업량

| 영역 | 파일 수 | 예상 시간 |
|-----|--------|----------|
| 인증 미들웨어 | 3-5 | 2-3일 |
| DB 마이그레이션 | 10-15 | 3-5일 |
| 워크스페이스 격리 | 5-7 | 2-3일 |
| 설정 저장소 변경 | 3-4 | 1-2일 |
| 서비스 user_id 전파 | 15-20 | 3-5일 |
| K8s 매니페스트 | 5-7 | 1-2일 |
| Docker 이미지 | 2-3 | 1일 |
| 테스트 | - | 3-5일 |
| **총계** | **~50-60** | **3-4주** |

---

## 7. 리스크 및 고려사항

### 7.1 기술적 리스크

| 리스크 | 영향도 | 대응 방안 |
|-------|--------|----------|
| PTY 세션 메모리 누수 | 높음 | 세션 타임아웃 및 자동 정리 |
| 파일시스템 권한 충돌 | 중간 | 일관된 UID/GID 사용 |
| WebSocket 연결 끊김 | 중간 | 재연결 로직 및 세션 복구 |
| Git 작업 충돌 | 낮음 | 락 메커니즘 및 재시도 |

### 7.2 운영 고려사항

- **스케일링**: 워크스페이스 데이터가 PVC에 저장되므로 Pod 스케일링 시 EFS(ReadWriteMany) 필요
- **백업**: PostgreSQL 및 EFS 정기 백업 설정
- **모니터링**: PTY 세션 수, 디스크 사용량, 메모리 모니터링
- **정리**: 오래된 워크스페이스 자동 정리 크론잡

---

## 8. 참고 자료

### 8.1 관련 파일

- `crates/local-deployment/src/lib.rs` - 메인 배포 구조체
- `crates/db/src/lib.rs` - 데이터베이스 서비스
- `crates/local-deployment/src/pty.rs` - PTY 서비스
- `crates/services/src/services/git.rs` - Git 서비스
- `crates/local-deployment/src/container.rs` - 컨테이너 서비스
- `crates/services/src/services/workspace_manager.rs` - 워크스페이스 관리
- `crates/server/src/routes/mod.rs` - API 라우트

### 8.2 기존 Remote 배포 참고

- `crates/remote/src/` - Remote 서버 구현
- `crates/remote/src/db/auth.rs` - PostgreSQL 기반 인증
- `k8s/` - 기존 K8s 매니페스트

---

*문서 작성일: 2025-01-21*
*작성자: Claude Code*
