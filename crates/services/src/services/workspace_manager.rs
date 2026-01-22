use std::path::{Path, PathBuf};

use db::models::{repo::Repo, workspace::Workspace as DbWorkspace};
use db::DeploymentMode;
use sqlx::{Pool, Sqlite};
use thiserror::Error;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::worktree_manager::{WorktreeCleanup, WorktreeError, WorktreeManager};

#[derive(Debug, Clone)]
pub struct RepoWorkspaceInput {
    pub repo: Repo,
    pub target_branch: String,
}

impl RepoWorkspaceInput {
    pub fn new(repo: Repo, target_branch: String) -> Self {
        Self {
            repo,
            target_branch,
        }
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No repositories provided")]
    NoRepositories,
    #[error("Partial workspace creation failed: {0}")]
    PartialCreation(String),
    #[error("Unauthorized: path {0} is outside user workspace boundary")]
    Unauthorized(String),
}

/// Info about a single repo's worktree within a workspace
#[derive(Debug, Clone)]
pub struct RepoWorktree {
    pub repo_id: Uuid,
    pub repo_name: String,
    pub source_repo_path: PathBuf,
    pub worktree_path: PathBuf,
}

/// A container directory holding worktrees for all project repos
#[derive(Debug, Clone)]
pub struct WorktreeContainer {
    pub workspace_dir: PathBuf,
    pub worktrees: Vec<RepoWorktree>,
}

pub struct WorkspaceManager;

impl WorkspaceManager {
    /// Create a workspace with worktrees for all repositories.
    /// On failure, rolls back any already-created worktrees.
    pub async fn create_workspace(
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<WorktreeContainer, WorkspaceError> {
        if repos.is_empty() {
            return Err(WorkspaceError::NoRepositories);
        }

        info!(
            "Creating workspace at {} with {} repositories",
            workspace_dir.display(),
            repos.len()
        );

        tokio::fs::create_dir_all(workspace_dir).await?;

        let mut created_worktrees: Vec<RepoWorktree> = Vec::new();

        for input in repos {
            let worktree_path = workspace_dir.join(&input.repo.name);

            debug!(
                "Creating worktree for repo '{}' at {}",
                input.repo.name,
                worktree_path.display()
            );

            match WorktreeManager::create_worktree(
                &input.repo.path,
                branch_name,
                &worktree_path,
                &input.target_branch,
                true,
            )
            .await
            {
                Ok(()) => {
                    created_worktrees.push(RepoWorktree {
                        repo_id: input.repo.id,
                        repo_name: input.repo.name.clone(),
                        source_repo_path: input.repo.path.clone(),
                        worktree_path,
                    });
                }
                Err(e) => {
                    error!(
                        "Failed to create worktree for repo '{}': {}. Rolling back...",
                        input.repo.name, e
                    );

                    // Rollback: cleanup all worktrees we've created so far
                    Self::cleanup_created_worktrees(&created_worktrees).await;

                    // Also remove the workspace directory if it's empty
                    if let Err(cleanup_err) = tokio::fs::remove_dir(workspace_dir).await {
                        debug!(
                            "Could not remove workspace dir during rollback: {}",
                            cleanup_err
                        );
                    }

                    return Err(WorkspaceError::PartialCreation(format!(
                        "Failed to create worktree for repo '{}': {}",
                        input.repo.name, e
                    )));
                }
            }
        }

        info!(
            "Successfully created workspace with {} worktrees",
            created_worktrees.len()
        );

        Ok(WorktreeContainer {
            workspace_dir: workspace_dir.to_path_buf(),
            worktrees: created_worktrees,
        })
    }

    /// Ensure all worktrees in a workspace exist (for cold restart scenarios)
    pub async fn ensure_workspace_exists(
        workspace_dir: &Path,
        repos: &[Repo],
        branch_name: &str,
    ) -> Result<(), WorkspaceError> {
        if repos.is_empty() {
            return Err(WorkspaceError::NoRepositories);
        }

        // Try legacy migration first (single repo projects only)
        // Old layout had worktree directly at workspace_dir; new layout has it at workspace_dir/{repo_name}
        if repos.len() == 1 && Self::migrate_legacy_worktree(workspace_dir, &repos[0]).await? {
            return Ok(());
        }

        if !workspace_dir.exists() {
            tokio::fs::create_dir_all(workspace_dir).await?;
        }

        for repo in repos {
            let worktree_path = workspace_dir.join(&repo.name);

            debug!(
                "Ensuring worktree exists for repo '{}' at {}",
                repo.name,
                worktree_path.display()
            );

            WorktreeManager::ensure_worktree_exists(&repo.path, branch_name, &worktree_path)
                .await?;
        }

        Ok(())
    }

    /// Clean up all worktrees in a workspace
    pub async fn cleanup_workspace(
        workspace_dir: &Path,
        repos: &[Repo],
    ) -> Result<(), WorkspaceError> {
        info!("Cleaning up workspace at {}", workspace_dir.display());

        let cleanup_data: Vec<WorktreeCleanup> = repos
            .iter()
            .map(|repo| {
                let worktree_path = workspace_dir.join(&repo.name);
                WorktreeCleanup::new(worktree_path, Some(repo.path.clone()))
            })
            .collect();

        WorktreeManager::batch_cleanup_worktrees(&cleanup_data).await?;

        // Remove the workspace directory itself
        if workspace_dir.exists()
            && let Err(e) = tokio::fs::remove_dir_all(workspace_dir).await
        {
            debug!(
                "Could not remove workspace directory {}: {}",
                workspace_dir.display(),
                e
            );
        }

        Ok(())
    }

    /// Get the base directory for workspaces (same as worktree base dir)
    pub fn get_workspace_base_dir() -> PathBuf {
        WorktreeManager::get_worktree_base_dir()
    }

    /// Get the base directory for workspaces for a specific user.
    ///
    /// In Kubernetes (multi-user) mode, returns `/workspaces/{user_id}/`
    /// or `{WORKSPACE_BASE_DIR}/{user_id}/` if the env var is set.
    ///
    /// In Desktop (single-user) mode, returns the same as `get_workspace_base_dir()`.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user whose workspace directory to return
    ///
    /// # Returns
    ///
    /// A `PathBuf` pointing to the user's workspace base directory.
    pub fn get_workspace_base_dir_for_user(user_id: &Uuid) -> PathBuf {
        let mode = DeploymentMode::detect();

        if mode.is_kubernetes() {
            // In K8s mode, use user-specific subdirectory
            let base = std::env::var("WORKSPACE_BASE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/workspaces"));
            base.join(user_id.to_string())
        } else {
            // In desktop mode, use the same directory for all (single user)
            Self::get_workspace_base_dir()
        }
    }

    /// Validate that a given path is within the user's workspace boundary.
    ///
    /// This function prevents path traversal attacks and ensures users can only
    /// access files within their designated workspace directories.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    /// * `path` - The path to validate
    ///
    /// # Returns
    ///
    /// Returns `Ok(canonicalized_path)` if the path is within the user's workspace.
    /// Returns `Err(WorkspaceError::Unauthorized)` if the path is outside the boundary.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let user_id = Uuid::new_v4();
    /// let path = Path::new("/workspaces/user-uuid/project");
    /// let validated = WorkspaceManager::validate_user_path(&user_id, path)?;
    /// ```
    pub fn validate_user_path(user_id: &Uuid, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let mode = DeploymentMode::detect();

        // In desktop mode, skip validation (single-user, no isolation needed)
        if mode.is_desktop() {
            // Still canonicalize if the path exists, otherwise return as-is
            return Ok(dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
        }

        // In K8s mode, enforce strict path validation
        let user_base = Self::get_workspace_base_dir_for_user(user_id);

        // Canonicalize the user's base directory (create if needed for validation)
        let canonical_base = if user_base.exists() {
            dunce::canonicalize(&user_base)?
        } else {
            // If base doesn't exist yet, use the path as-is for comparison
            user_base.clone()
        };

        // Canonicalize the target path
        // If the path doesn't exist, we need to resolve it relative to detect traversal
        let canonical_path = if path.exists() {
            dunce::canonicalize(path)?
        } else {
            // For non-existent paths, resolve parent components to detect traversal
            let mut resolved = PathBuf::new();
            for component in path.components() {
                resolved.push(component);
                // Try to canonicalize what exists so far
                if resolved.exists() {
                    resolved = dunce::canonicalize(&resolved)?;
                }
            }
            resolved
        };

        // Verify the canonical path starts with the user's base directory
        if canonical_path.starts_with(&canonical_base) {
            Ok(canonical_path)
        } else {
            warn!(
                user_id = %user_id,
                requested_path = %path.display(),
                canonical_path = %canonical_path.display(),
                user_base = %canonical_base.display(),
                "Path traversal attempt detected"
            );
            Err(WorkspaceError::Unauthorized(path.display().to_string()))
        }
    }

    /// Create a workspace with worktrees for all repositories, with user-aware path validation.
    ///
    /// In Kubernetes mode, validates that workspace_dir is within the user's workspace boundary.
    /// In Desktop mode, behaves the same as the original create_workspace.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user creating the workspace
    /// * `workspace_dir` - The directory where the workspace will be created
    /// * `repos` - The repositories to include in the workspace
    /// * `branch_name` - The name of the branch to create worktrees on
    ///
    /// # Returns
    ///
    /// Returns a `WorktreeContainer` on success, or a `WorkspaceError` on failure.
    pub async fn create_workspace_for_user(
        user_id: &Uuid,
        workspace_dir: &Path,
        repos: &[RepoWorkspaceInput],
        branch_name: &str,
    ) -> Result<WorktreeContainer, WorkspaceError> {
        // Validate path is within user's workspace boundary
        Self::validate_user_path(user_id, workspace_dir)?;

        // Ensure user's base directory exists
        let user_base = Self::get_workspace_base_dir_for_user(user_id);
        tokio::fs::create_dir_all(&user_base).await?;

        // Delegate to existing create_workspace logic
        Self::create_workspace(workspace_dir, repos, branch_name).await
    }

    /// Ensure all worktrees in a workspace exist, with user-aware path validation.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    /// * `workspace_dir` - The workspace directory
    /// * `repos` - The repositories in the workspace
    /// * `branch_name` - The branch name for worktrees
    pub async fn ensure_workspace_exists_for_user(
        user_id: &Uuid,
        workspace_dir: &Path,
        repos: &[Repo],
        branch_name: &str,
    ) -> Result<(), WorkspaceError> {
        // Validate path is within user's workspace boundary
        Self::validate_user_path(user_id, workspace_dir)?;

        // Delegate to existing ensure_workspace_exists logic
        Self::ensure_workspace_exists(workspace_dir, repos, branch_name).await
    }

    /// Clean up all worktrees in a workspace, with user-aware path validation.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    /// * `workspace_dir` - The workspace directory
    /// * `repos` - The repositories in the workspace
    pub async fn cleanup_workspace_for_user(
        user_id: &Uuid,
        workspace_dir: &Path,
        repos: &[Repo],
    ) -> Result<(), WorkspaceError> {
        // Validate path is within user's workspace boundary
        Self::validate_user_path(user_id, workspace_dir)?;

        // Delegate to existing cleanup_workspace logic
        Self::cleanup_workspace(workspace_dir, repos).await
    }

    /// Migrate a legacy single-worktree layout to the new workspace layout.
    /// Old layout: workspace_dir IS the worktree
    /// New layout: workspace_dir contains worktrees at workspace_dir/{repo_name}
    ///
    /// Returns Ok(true) if migration was performed, Ok(false) if no migration needed.
    pub async fn migrate_legacy_worktree(
        workspace_dir: &Path,
        repo: &Repo,
    ) -> Result<bool, WorkspaceError> {
        let expected_worktree_path = workspace_dir.join(&repo.name);

        // Detect old-style: workspace_dir exists AND has .git file (worktree marker)
        // AND expected new location doesn't exist
        let git_file = workspace_dir.join(".git");
        let is_old_style = workspace_dir.exists()
            && git_file.exists()
            && git_file.is_file() // .git file = worktree, .git dir = main repo
            && !expected_worktree_path.exists();

        if !is_old_style {
            return Ok(false);
        }

        info!(
            "Detected legacy worktree at {}, migrating to new layout",
            workspace_dir.display()
        );

        // Move old worktree to temp location (can't move into subdirectory of itself)
        let temp_name = format!(
            "{}-migrating",
            workspace_dir
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        );
        let temp_path = workspace_dir.with_file_name(temp_name);

        WorktreeManager::move_worktree(&repo.path, workspace_dir, &temp_path).await?;

        // Create new workspace directory
        tokio::fs::create_dir_all(workspace_dir).await?;

        // Move worktree to final location using git worktree move
        WorktreeManager::move_worktree(&repo.path, &temp_path, &expected_worktree_path).await?;

        if temp_path.exists() {
            let _ = tokio::fs::remove_dir_all(&temp_path).await;
        }

        info!(
            "Successfully migrated legacy worktree to {}",
            expected_worktree_path.display()
        );

        Ok(true)
    }

    /// Helper to cleanup worktrees during rollback
    async fn cleanup_created_worktrees(worktrees: &[RepoWorktree]) {
        for worktree in worktrees {
            let cleanup = WorktreeCleanup::new(
                worktree.worktree_path.clone(),
                Some(worktree.source_repo_path.clone()),
            );

            if let Err(e) = WorktreeManager::cleanup_worktree(&cleanup).await {
                error!(
                    "Failed to cleanup worktree '{}' during rollback: {}",
                    worktree.repo_name, e
                );
            }
        }
    }

    pub async fn cleanup_orphan_workspaces(db: &Pool<Sqlite>) {
        if std::env::var("DISABLE_WORKTREE_ORPHAN_CLEANUP").is_ok() {
            debug!(
                "Orphan workspace cleanup is disabled via DISABLE_WORKTREE_ORPHAN_CLEANUP environment variable"
            );
            return;
        }

        // Always clean up the default directory
        let default_dir = WorktreeManager::get_default_worktree_base_dir();
        Self::cleanup_orphans_in_directory(db, &default_dir).await;

        // Also clean up custom directory if it's different from the default
        let current_dir = Self::get_workspace_base_dir();
        if current_dir != default_dir {
            Self::cleanup_orphans_in_directory(db, &current_dir).await;
        }
    }

    async fn cleanup_orphans_in_directory(db: &Pool<Sqlite>, workspace_base_dir: &Path) {
        if !workspace_base_dir.exists() {
            debug!(
                "Workspace base directory {} does not exist, skipping orphan cleanup",
                workspace_base_dir.display()
            );
            return;
        }

        let entries = match std::fs::read_dir(workspace_base_dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!(
                    "Failed to read workspace base directory {}: {}",
                    workspace_base_dir.display(),
                    e
                );
                return;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    warn!("Failed to read directory entry: {}", e);
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let workspace_path_str = path.to_string_lossy().to_string();
            if let Ok(false) = DbWorkspace::container_ref_exists(db, &workspace_path_str).await {
                info!("Found orphaned workspace: {}", workspace_path_str);
                if let Err(e) = Self::cleanup_workspace_without_repos(&path).await {
                    error!(
                        "Failed to remove orphaned workspace {}: {}",
                        workspace_path_str, e
                    );
                } else {
                    info!(
                        "Successfully removed orphaned workspace: {}",
                        workspace_path_str
                    );
                }
            }
        }
    }

    async fn cleanup_workspace_without_repos(workspace_dir: &Path) -> Result<(), WorkspaceError> {
        info!(
            "Cleaning up orphaned workspace at {}",
            workspace_dir.display()
        );

        let entries = match std::fs::read_dir(workspace_dir) {
            Ok(entries) => entries,
            Err(e) => {
                debug!(
                    "Cannot read workspace directory {}, attempting direct removal: {}",
                    workspace_dir.display(),
                    e
                );
                return tokio::fs::remove_dir_all(workspace_dir)
                    .await
                    .map_err(WorkspaceError::Io);
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir()
                && let Err(e) = WorktreeManager::cleanup_suspected_worktree(&path).await
            {
                warn!("Failed to cleanup suspected worktree: {}", e);
            }
        }

        if workspace_dir.exists()
            && let Err(e) = tokio::fs::remove_dir_all(workspace_dir).await
        {
            debug!(
                "Could not remove workspace directory {}: {}",
                workspace_dir.display(),
                e
            );
        }

        Ok(())
    }
}
