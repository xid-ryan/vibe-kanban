use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use db::{DBService, DBServicePg, DeploymentMode};
use deployment::{Deployment, DeploymentError, RemoteClientNotConfigured};
use executors::profile::ExecutorConfigs;
use services::services::{
    analytics::{AnalyticsConfig, AnalyticsContext, AnalyticsService, generate_user_id},
    approvals::Approvals,
    auth::AuthContext,
    config::{Config, load_config_from_file, save_config_to_file},
    config_db::ConfigServicePg,
    container::ContainerService,
    events::EventService,
    file_search::FileSearchCache,
    filesystem::FilesystemService,
    git::GitService,
    image::ImageService,
    oauth_credentials::OAuthCredentials,
    project::ProjectService,
    queued_message::QueuedMessageService,
    remote_client::{RemoteClient, RemoteClientError},
    repo::RepoService,
    worktree_manager::WorktreeManager,
};
use tokio::sync::RwLock;
use utils::{
    api::oauth::LoginStatus,
    assets::{config_path, credentials_path},
    msg_store::MsgStore,
};
use uuid::Uuid;

use crate::{container::LocalContainerService, pty::PtyService};
mod command;
pub mod container;
mod copy;
pub mod pty;
mod cleanup;

/// Database backend abstraction for supporting both SQLite (desktop) and PostgreSQL (K8s) modes.
///
/// In desktop mode, SQLite is used for local storage. In Kubernetes mode, PostgreSQL is used
/// for multi-user data isolation and shared access across pods.
#[derive(Clone)]
pub enum DbBackend {
    /// SQLite database for single-user desktop mode.
    Sqlite(DBService),
    /// PostgreSQL database for multi-user Kubernetes mode.
    Postgres(DBServicePg),
}

impl DbBackend {
    /// Get the SQLite pool if in desktop mode.
    ///
    /// # Returns
    ///
    /// `Some(&DBService)` if in desktop mode, `None` otherwise.
    pub fn as_sqlite(&self) -> Option<&DBService> {
        match self {
            DbBackend::Sqlite(db) => Some(db),
            DbBackend::Postgres(_) => None,
        }
    }

    /// Get the PostgreSQL pool if in K8s mode.
    ///
    /// # Returns
    ///
    /// `Some(&DBServicePg)` if in K8s mode, `None` otherwise.
    pub fn as_postgres(&self) -> Option<&DBServicePg> {
        match self {
            DbBackend::Sqlite(_) => None,
            DbBackend::Postgres(db) => Some(db),
        }
    }

    /// Check if using SQLite backend.
    pub fn is_sqlite(&self) -> bool {
        matches!(self, DbBackend::Sqlite(_))
    }

    /// Check if using PostgreSQL backend.
    pub fn is_postgres(&self) -> bool {
        matches!(self, DbBackend::Postgres(_))
    }
}

/// Configuration backend abstraction for supporting both file-based (desktop) and
/// database-backed (K8s) configuration storage.
#[derive(Clone)]
pub enum ConfigBackend {
    /// File-based configuration for desktop mode.
    File,
    /// Database-backed configuration for K8s multi-user mode.
    Database(ConfigServicePg),
}

impl ConfigBackend {
    /// Get the database-backed config service if in K8s mode.
    pub fn as_database(&self) -> Option<&ConfigServicePg> {
        match self {
            ConfigBackend::File => None,
            ConfigBackend::Database(svc) => Some(svc),
        }
    }

    /// Check if using file-based configuration.
    pub fn is_file_based(&self) -> bool {
        matches!(self, ConfigBackend::File)
    }

    /// Check if using database-backed configuration.
    pub fn is_database_backed(&self) -> bool {
        matches!(self, ConfigBackend::Database(_))
    }
}

#[derive(Clone)]
pub struct LocalDeployment {
    /// The deployment mode (desktop or Kubernetes).
    mode: DeploymentMode,
    config: Arc<RwLock<Config>>,
    user_id: String,
    /// SQLite database service (used in desktop mode and for local caching in K8s mode).
    db: DBService,
    /// Database backend abstraction for user data.
    db_backend: DbBackend,
    /// Configuration backend (file-based or database-backed).
    config_backend: ConfigBackend,
    analytics: Option<AnalyticsService>,
    container: LocalContainerService,
    git: GitService,
    project: ProjectService,
    repo: RepoService,
    image: ImageService,
    filesystem: FilesystemService,
    events: EventService,
    file_search_cache: Arc<FileSearchCache>,
    approvals: Approvals,
    queued_message_service: QueuedMessageService,
    remote_client: Result<RemoteClient, RemoteClientNotConfigured>,
    auth_context: AuthContext,
    oauth_handoffs: Arc<RwLock<HashMap<Uuid, PendingHandoff>>>,
    pty: PtyService,
}

#[derive(Debug, Clone)]
struct PendingHandoff {
    provider: String,
    app_verifier: String,
}

#[async_trait]
impl Deployment for LocalDeployment {
    async fn new() -> Result<Self, DeploymentError> {
        // Detect deployment mode from environment
        let mode = DeploymentMode::detect();
        tracing::info!(
            mode = mode.as_str(),
            database = mode.expected_database(),
            "Initializing LocalDeployment"
        );

        // Load configuration based on deployment mode
        let (raw_config, config_backend) = if mode.is_kubernetes() {
            // In K8s mode, we need PostgreSQL for the config service
            // Initialize PostgreSQL first to load config from database
            let pg_db = DBServicePg::new().await.map_err(|e| {
                tracing::error!(?e, "Failed to initialize PostgreSQL database");
                DeploymentError::DbInit(e.to_string())
            })?;
            let config_service = ConfigServicePg::new(pg_db.pool.clone());

            // Note: In K8s mode, user_id comes from JWT token, not generated locally.
            // For initialization, we use a default config. The actual user config
            // will be loaded per-request using the authenticated user's ID.
            let raw_config = Config::default();
            tracing::info!("K8s mode: Using database-backed configuration");

            (raw_config, ConfigBackend::Database(config_service))
        } else {
            // In desktop mode, load config from file
            let mut raw_config = load_config_from_file(&config_path()).await;

            let profiles = ExecutorConfigs::get_cached();
            if !raw_config.onboarding_acknowledged
                && let Ok(recommended_executor) = profiles.get_recommended_executor_profile().await
            {
                raw_config.executor_profile = recommended_executor;
            }

            // Check if app version has changed and set release notes flag
            {
                let current_version = utils::version::APP_VERSION;
                let stored_version = raw_config.last_app_version.as_deref();

                if stored_version != Some(current_version) {
                    // Show release notes only if this is an upgrade (not first install)
                    raw_config.show_release_notes = stored_version.is_some();
                    raw_config.last_app_version = Some(current_version.to_string());
                }
            }

            // Always save config (may have been migrated or version updated)
            save_config_to_file(&raw_config, &config_path()).await?;
            tracing::info!("Desktop mode: Using file-based configuration");

            (raw_config, ConfigBackend::File)
        };

        if let Some(workspace_dir) = &raw_config.workspace_dir {
            let path = utils::path::expand_tilde(workspace_dir);
            WorktreeManager::set_workspace_dir_override(path);
        }

        let config = Arc::new(RwLock::new(raw_config));
        let user_id = generate_user_id();
        let analytics = AnalyticsConfig::new().map(AnalyticsService::new);
        let git = GitService::new();
        let project = ProjectService::new();
        let repo = RepoService::new();
        let msg_stores = Arc::new(RwLock::new(HashMap::new()));
        let filesystem = FilesystemService::new();

        // Create shared components for EventService
        let events_msg_store = Arc::new(MsgStore::new());
        let events_entry_count = Arc::new(RwLock::new(0));

        // Initialize database backends based on deployment mode
        let (db, db_backend) = if mode.is_kubernetes() {
            // In K8s mode, use PostgreSQL for user data
            let pg_db = DBServicePg::new().await.map_err(|e| {
                tracing::error!(?e, "Failed to initialize PostgreSQL database");
                DeploymentError::DbInit(e.to_string())
            })?;

            // We still need SQLite for local operations (EventService hooks, ImageService)
            // Create a local SQLite database for caching and local operations
            let sqlite_db = {
                let hook = EventService::create_hook(
                    events_msg_store.clone(),
                    events_entry_count.clone(),
                    DBService::new().await?, // Temporary DB service for the hook
                );
                DBService::new_with_after_connect(hook).await?
            };

            tracing::info!("K8s mode: Using PostgreSQL for user data, SQLite for local cache");
            (sqlite_db, DbBackend::Postgres(pg_db))
        } else {
            // In desktop mode, use SQLite for everything
            let db = {
                let hook = EventService::create_hook(
                    events_msg_store.clone(),
                    events_entry_count.clone(),
                    DBService::new().await?, // Temporary DB service for the hook
                );
                DBService::new_with_after_connect(hook).await?
            };

            tracing::info!("Desktop mode: Using SQLite database");
            (db.clone(), DbBackend::Sqlite(db))
        };

        let image = ImageService::new(db.clone().pool)?;
        {
            let image_service = image.clone();
            tokio::spawn(async move {
                tracing::info!("Starting orphaned image cleanup...");
                if let Err(e) = image_service.delete_orphaned_images().await {
                    tracing::error!("Failed to clean up orphaned images: {}", e);
                }
            });
        }

        let approvals = Approvals::new(msg_stores.clone());
        let queued_message_service = QueuedMessageService::new();

        let oauth_credentials = Arc::new(OAuthCredentials::new(credentials_path()));
        if let Err(e) = oauth_credentials.load().await {
            tracing::warn!(?e, "failed to load OAuth credentials");
        }

        let profile_cache = Arc::new(RwLock::new(None));
        let auth_context = AuthContext::new(oauth_credentials.clone(), profile_cache.clone());

        let api_base = std::env::var("VK_SHARED_API_BASE")
            .ok()
            .or_else(|| option_env!("VK_SHARED_API_BASE").map(|s| s.to_string()));

        let remote_client = match api_base {
            Some(url) => match RemoteClient::new(&url, auth_context.clone()) {
                Ok(client) => {
                    tracing::info!("Remote client initialized with URL: {}", url);
                    Ok(client)
                }
                Err(e) => {
                    tracing::error!(?e, "failed to create remote client");
                    Err(RemoteClientNotConfigured)
                }
            },
            None => {
                tracing::info!("VK_SHARED_API_BASE not set; remote features disabled");
                Err(RemoteClientNotConfigured)
            }
        };

        let oauth_handoffs = Arc::new(RwLock::new(HashMap::new()));

        // We need to make analytics accessible to the ContainerService
        // TODO: Handle this more gracefully
        let analytics_ctx = analytics.as_ref().map(|s| AnalyticsContext {
            user_id: user_id.clone(),
            analytics_service: s.clone(),
        });
        let container = LocalContainerService::new(
            db.clone(),
            msg_stores.clone(),
            config.clone(),
            git.clone(),
            image.clone(),
            analytics_ctx,
            approvals.clone(),
            queued_message_service.clone(),
        )
        .await;

        let events = EventService::new(db.clone(), events_msg_store, events_entry_count);

        let file_search_cache = Arc::new(FileSearchCache::new());

        let pty = PtyService::new();

        // Spawn the resource cleanup job for PTY sessions and orphaned processes
        {
            let pty_service = pty.clone();
            let container_service = container.clone();
            let cleanup_config = cleanup::CleanupConfig::from_env();
            cleanup::spawn_cleanup_job(pty_service, container_service, cleanup_config);
        }

        let deployment = Self {
            mode,
            config,
            user_id,
            db,
            db_backend,
            config_backend,
            analytics,
            container,
            git,
            project,
            repo,
            image,
            filesystem,
            events,
            file_search_cache,
            approvals,
            queued_message_service,
            remote_client,
            auth_context,
            oauth_handoffs,
            pty,
        };

        Ok(deployment)
    }

    fn user_id(&self) -> &str {
        &self.user_id
    }

    fn config(&self) -> &Arc<RwLock<Config>> {
        &self.config
    }

    fn db(&self) -> &DBService {
        &self.db
    }

    fn analytics(&self) -> &Option<AnalyticsService> {
        &self.analytics
    }

    fn container(&self) -> &impl ContainerService {
        &self.container
    }

    fn git(&self) -> &GitService {
        &self.git
    }

    fn project(&self) -> &ProjectService {
        &self.project
    }

    fn repo(&self) -> &RepoService {
        &self.repo
    }

    fn image(&self) -> &ImageService {
        &self.image
    }

    fn filesystem(&self) -> &FilesystemService {
        &self.filesystem
    }

    fn events(&self) -> &EventService {
        &self.events
    }

    fn file_search_cache(&self) -> &Arc<FileSearchCache> {
        &self.file_search_cache
    }

    fn approvals(&self) -> &Approvals {
        &self.approvals
    }

    fn queued_message_service(&self) -> &QueuedMessageService {
        &self.queued_message_service
    }

    fn auth_context(&self) -> &AuthContext {
        &self.auth_context
    }
}

impl LocalDeployment {
    pub fn remote_client(&self) -> Result<RemoteClient, RemoteClientNotConfigured> {
        self.remote_client.clone()
    }

    pub async fn get_login_status(&self) -> LoginStatus {
        if self.auth_context.get_credentials().await.is_none() {
            self.auth_context.clear_profile().await;
            return LoginStatus::LoggedOut;
        };

        if let Some(cached_profile) = self.auth_context.cached_profile().await {
            return LoginStatus::LoggedIn {
                profile: cached_profile,
            };
        }

        let Ok(client) = self.remote_client() else {
            return LoginStatus::LoggedOut;
        };

        match client.profile().await {
            Ok(profile) => {
                self.auth_context.set_profile(profile.clone()).await;
                LoginStatus::LoggedIn { profile }
            }
            Err(RemoteClientError::Auth) => {
                let _ = self.auth_context.clear_credentials().await;
                self.auth_context.clear_profile().await;
                LoginStatus::LoggedOut
            }
            Err(_) => LoginStatus::LoggedOut,
        }
    }

    pub async fn store_oauth_handoff(
        &self,
        handoff_id: Uuid,
        provider: String,
        app_verifier: String,
    ) {
        self.oauth_handoffs.write().await.insert(
            handoff_id,
            PendingHandoff {
                provider,
                app_verifier,
            },
        );
    }

    pub async fn take_oauth_handoff(&self, handoff_id: &Uuid) -> Option<(String, String)> {
        self.oauth_handoffs
            .write()
            .await
            .remove(handoff_id)
            .map(|state| (state.provider, state.app_verifier))
    }

    pub fn pty(&self) -> &PtyService {
        &self.pty
    }

    // ===== Deployment Mode Helpers =====

    /// Get the current deployment mode.
    ///
    /// # Returns
    ///
    /// The `DeploymentMode` indicating whether we're running in desktop or Kubernetes mode.
    pub fn mode(&self) -> DeploymentMode {
        self.mode
    }

    /// Check if the application is running in Kubernetes (multi-user) mode.
    ///
    /// In K8s mode:
    /// - PostgreSQL is used for user data
    /// - JWT authentication is required
    /// - User workspace isolation is enforced
    /// - Configuration is stored in the database
    ///
    /// # Returns
    ///
    /// `true` if running in Kubernetes mode, `false` if running in desktop mode.
    pub fn is_k8s_mode(&self) -> bool {
        self.mode.is_kubernetes()
    }

    /// Check if the application is running in desktop (single-user) mode.
    ///
    /// In desktop mode:
    /// - SQLite is used for all data
    /// - No authentication is required
    /// - No workspace isolation
    /// - Configuration is stored in local files
    ///
    /// # Returns
    ///
    /// `true` if running in desktop mode, `false` if running in Kubernetes mode.
    pub fn is_desktop_mode(&self) -> bool {
        self.mode.is_desktop()
    }

    /// Get the database backend abstraction.
    ///
    /// This provides access to either SQLite or PostgreSQL depending on the deployment mode.
    ///
    /// # Returns
    ///
    /// A reference to the `DbBackend` enum.
    pub fn db_backend(&self) -> &DbBackend {
        &self.db_backend
    }

    /// Get the PostgreSQL database service if in K8s mode.
    ///
    /// # Returns
    ///
    /// `Some(&DBServicePg)` if in Kubernetes mode, `None` if in desktop mode.
    pub fn pg_db(&self) -> Option<&DBServicePg> {
        self.db_backend.as_postgres()
    }

    /// Get the configuration backend.
    ///
    /// This provides access to either file-based or database-backed configuration
    /// depending on the deployment mode.
    ///
    /// # Returns
    ///
    /// A reference to the `ConfigBackend` enum.
    pub fn config_backend(&self) -> &ConfigBackend {
        &self.config_backend
    }

    /// Get the database-backed config service if in K8s mode.
    ///
    /// # Returns
    ///
    /// `Some(&ConfigServicePg)` if in Kubernetes mode, `None` if in desktop mode.
    pub fn config_service(&self) -> Option<&ConfigServicePg> {
        self.config_backend.as_database()
    }

    /// Check if authentication should be required for requests.
    ///
    /// Authentication is only required in Kubernetes multi-user mode.
    ///
    /// # Returns
    ///
    /// `true` if JWT authentication should be enforced, `false` otherwise.
    pub fn requires_auth(&self) -> bool {
        self.mode.is_kubernetes()
    }
}
