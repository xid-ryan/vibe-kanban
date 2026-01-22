#[cfg(not(feature = "qa-mode"))]
use std::collections::HashSet;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(not(feature = "qa-mode"))]
use ignore::WalkBuilder;
use serde::Serialize;
use thiserror::Error;
#[cfg(not(feature = "qa-mode"))]
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use uuid::Uuid;

use super::workspace_manager::{WorkspaceError, WorkspaceManager};

#[derive(Clone)]
pub struct FilesystemService {}

#[derive(Debug, Error)]
pub enum FilesystemError {
    #[error("Directory does not exist")]
    DirectoryDoesNotExist,
    #[error("Path is not a directory")]
    PathIsNotDirectory,
    #[error("Failed to read directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unauthorized: path {0} is outside user workspace boundary")]
    Unauthorized(String),
}

impl From<WorkspaceError> for FilesystemError {
    fn from(err: WorkspaceError) -> Self {
        match err {
            WorkspaceError::Unauthorized(path) => FilesystemError::Unauthorized(path),
            other => FilesystemError::Io(std::io::Error::other(other.to_string())),
        }
    }
}
#[derive(Debug, Serialize, TS)]
pub struct DirectoryListResponse {
    pub entries: Vec<DirectoryEntry>,
    pub current_path: String,
}

#[derive(Debug, Serialize, TS)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_directory: bool,
    pub is_git_repo: bool,
    pub last_modified: Option<u64>,
}

impl Default for FilesystemService {
    fn default() -> Self {
        Self::new()
    }
}

impl FilesystemService {
    pub fn new() -> Self {
        FilesystemService {}
    }

    #[cfg(not(feature = "qa-mode"))]
    fn get_directories_to_skip() -> HashSet<String> {
        let mut skip_dirs = HashSet::from(
            [
                "node_modules",
                "target",
                "build",
                "dist",
                ".next",
                ".nuxt",
                ".cache",
                ".npm",
                ".yarn",
                ".pnpm-store",
                "Library",
                "AppData",
                "Applications",
            ]
            .map(String::from),
        );

        [
            dirs::executable_dir(),
            dirs::data_dir(),
            dirs::download_dir(),
            dirs::picture_dir(),
            dirs::video_dir(),
            dirs::audio_dir(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|path| path.file_name()?.to_str().map(String::from))
        .for_each(|name| {
            skip_dirs.insert(name);
        });

        skip_dirs
    }

    #[cfg_attr(feature = "qa-mode", allow(unused_variables))]
    pub async fn list_git_repos(
        &self,
        path: Option<String>,
        timeout_ms: u64,
        hard_timeout_ms: u64,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        #[cfg(feature = "qa-mode")]
        {
            tracing::info!("QA mode: returning hardcoded QA repos instead of scanning filesystem");
            super::qa_repos::get_qa_repos()
        }

        #[cfg(not(feature = "qa-mode"))]
        {
            let base_path = path
                .map(PathBuf::from)
                .unwrap_or_else(Self::get_home_directory);
            Self::verify_directory(&base_path)?;
            self.list_git_repos_with_timeout(
                vec![base_path],
                timeout_ms,
                hard_timeout_ms,
                max_depth,
            )
            .await
        }
    }

    #[cfg(not(feature = "qa-mode"))]
    async fn list_git_repos_with_timeout(
        &self,
        paths: Vec<PathBuf>,
        timeout_ms: u64,
        hard_timeout_ms: u64,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        let cancel_token = CancellationToken::new();
        let cancel_after_delay = cancel_token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
            cancel_after_delay.cancel();
        });
        let service = self.clone();
        let cancel_for_scan = cancel_token.clone();
        let mut scan_handle = tokio::spawn(async move {
            service
                .list_git_repos_inner(paths, max_depth, Some(&cancel_for_scan))
                .await
        });

        let hard_timeout = tokio::time::sleep(std::time::Duration::from_millis(hard_timeout_ms));
        tokio::pin!(hard_timeout);

        tokio::select! {
            res = &mut scan_handle => {
                match res {
                    Ok(Ok(repos)) => Ok(repos),
                    Ok(Err(err)) => Err(err),
                    Err(join_err) => Err(FilesystemError::Io(
                        std::io::Error::other(join_err.to_string())))
                }
                }
            _ = &mut hard_timeout => {
                scan_handle.abort();
                tracing::warn!("list_git_repos_with_timeout: hard timeout reached after {}ms", hard_timeout_ms);
                Err(FilesystemError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Operation forcibly terminated due to hard timeout",
                )))
            }
        }
    }

    #[cfg_attr(feature = "qa-mode", allow(unused_variables))]
    pub async fn list_common_git_repos(
        &self,
        timeout_ms: u64,
        hard_timeout_ms: u64,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        #[cfg(feature = "qa-mode")]
        {
            tracing::info!(
                "QA mode: returning hardcoded QA repos instead of scanning common directories"
            );
            super::qa_repos::get_qa_repos()
        }

        #[cfg(not(feature = "qa-mode"))]
        {
            let search_strings = ["repos", "dev", "work", "code", "projects"];
            let home_dir = Self::get_home_directory();
            let mut paths: Vec<PathBuf> = search_strings
                .iter()
                .map(|s| home_dir.join(s))
                .filter(|p| p.exists() && p.is_dir())
                .collect();
            paths.insert(0, home_dir);
            if let Some(cwd) = std::env::current_dir().ok()
                && cwd.exists()
                && cwd.is_dir()
            {
                paths.insert(0, cwd);
            }
            self.list_git_repos_with_timeout(paths, timeout_ms, hard_timeout_ms, max_depth)
                .await
        }
    }

    #[cfg(not(feature = "qa-mode"))]
    async fn list_git_repos_inner(
        &self,
        path: Vec<PathBuf>,
        max_depth: Option<usize>,
        cancel: Option<&CancellationToken>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        let base_dir = match path.first() {
            Some(dir) => dir,
            None => return Ok(vec![]),
        };
        let skip_dirs = Self::get_directories_to_skip();
        let vibe_kanban_temp_dir = utils::path::get_vibe_kanban_temp_dir();
        let mut walker_builder = WalkBuilder::new(base_dir);
        walker_builder
            .follow_links(false)
            .hidden(true) // true to skip hidden files
            .git_ignore(true)
            .filter_entry({
                let cancel = cancel.cloned();
                move |entry| {
                    if let Some(token) = cancel.as_ref()
                        && token.is_cancelled()
                    {
                        tracing::debug!("Cancellation token triggered");
                        return false;
                    }

                    let path = entry.path();
                    if !path.is_dir() {
                        return false;
                    }

                    // Skip vibe-kanban temp directory and all subdirectories
                    // Normalize to handle macOS /private/var vs /var aliasing
                    if utils::path::normalize_macos_private_alias(path)
                        .starts_with(&vibe_kanban_temp_dir)
                    {
                        return false;
                    }

                    // Skip common non-git folders
                    if let Some(name) = path.file_name().and_then(|n| n.to_str())
                        && skip_dirs.contains(name)
                    {
                        return false;
                    }

                    true
                }
            })
            .max_depth(max_depth)
            .git_exclude(true);
        for p in path.iter().skip(1) {
            walker_builder.add(p);
        }
        let mut seen_dirs = HashSet::new();
        let mut git_repos: Vec<DirectoryEntry> = walker_builder
            .build()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if seen_dirs.contains(entry.path()) {
                    return None;
                }
                seen_dirs.insert(entry.path().to_owned());
                let name = entry.file_name().to_str()?;
                if !entry.path().join(".git").exists() {
                    return None;
                }
                let last_modified = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|t| t.elapsed().unwrap_or_default().as_secs());
                Some(DirectoryEntry {
                    name: name.to_string(),
                    path: entry.into_path(),
                    is_directory: true,
                    is_git_repo: true,
                    last_modified,
                })
            })
            .collect();
        git_repos.sort_by_key(|entry| entry.last_modified.unwrap_or(0));
        Ok(git_repos)
    }

    fn get_home_directory() -> PathBuf {
        dirs::home_dir()
            .or_else(dirs::desktop_dir)
            .or_else(dirs::document_dir)
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    std::env::var("USERPROFILE")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| PathBuf::from("C:\\"))
                } else {
                    PathBuf::from("/")
                }
            })
    }

    fn verify_directory(path: &Path) -> Result<(), FilesystemError> {
        if !path.exists() {
            return Err(FilesystemError::DirectoryDoesNotExist);
        }
        if !path.is_dir() {
            return Err(FilesystemError::PathIsNotDirectory);
        }
        Ok(())
    }

    pub async fn list_directory(
        &self,
        path: Option<String>,
    ) -> Result<DirectoryListResponse, FilesystemError> {
        let path = path
            .map(PathBuf::from)
            .unwrap_or_else(Self::get_home_directory);
        Self::verify_directory(&path)?;

        let entries = fs::read_dir(&path)?;
        let mut directory_entries = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = entry.metadata().ok();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip hidden files/directories
                if name.starts_with('.') && name != ".." {
                    continue;
                }

                let is_directory = metadata.is_some_and(|m| m.is_dir());
                let is_git_repo = if is_directory {
                    path.join(".git").exists()
                } else {
                    false
                };

                directory_entries.push(DirectoryEntry {
                    name: name.to_string(),
                    path,
                    is_directory,
                    is_git_repo,
                    last_modified: None,
                });
            }
        }
        // Sort: directories first, then files, both alphabetically
        directory_entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(DirectoryListResponse {
            entries: directory_entries,
            current_path: path.to_string_lossy().to_string(),
        })
    }

    // =========================================================================
    // Multi-user support methods (Kubernetes mode)
    // =========================================================================

    /// Get the home directory for a specific user.
    ///
    /// In Kubernetes mode with multi-user support, this returns the user's
    /// isolated workspace directory. In Desktop mode (single-user),
    /// this falls back to the system home directory.
    ///
    /// # Arguments
    ///
    /// * `user_id` - Optional user UUID. If None, uses desktop mode fallback.
    ///
    /// # Returns
    ///
    /// Returns the user's home directory path.
    pub fn get_home_directory_for_user(user_id: Option<&Uuid>) -> PathBuf {
        match user_id {
            Some(id) => WorkspaceManager::get_workspace_base_dir_for_user(id),
            None => Self::get_home_directory(), // Desktop mode fallback
        }
    }

    /// Validate that a path is within the user's workspace boundary.
    ///
    /// This function prevents path traversal attacks and ensures users can only
    /// access files within their designated workspace directories.
    ///
    /// In Desktop mode, validation is skipped (single-user, no isolation needed).
    /// In Kubernetes mode, strict path validation is enforced.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    /// * `path` - The path to validate
    ///
    /// # Returns
    ///
    /// Returns `Ok(canonicalized_path)` if the path is within the user's workspace.
    /// Returns `Err(FilesystemError::Unauthorized)` if the path is outside the boundary.
    pub fn validate_path_for_user(
        &self,
        user_id: &Uuid,
        path: &Path,
    ) -> Result<PathBuf, FilesystemError> {
        WorkspaceManager::validate_user_path(user_id, path).map_err(FilesystemError::from)
    }

    /// List Git repositories with user-aware path restrictions.
    ///
    /// In Kubernetes mode, restricts the search to the user's workspace directory.
    /// In Desktop mode (user_id is None), searches from the specified path or
    /// system home directory.
    ///
    /// # Arguments
    ///
    /// * `user_id` - Optional user UUID for workspace restriction
    /// * `path` - Optional path override (validated against user workspace if user_id is provided)
    /// * `timeout_ms` - Soft timeout in milliseconds
    /// * `hard_timeout_ms` - Hard timeout in milliseconds
    /// * `max_depth` - Maximum directory depth to search
    #[cfg_attr(feature = "qa-mode", allow(unused_variables))]
    pub async fn list_git_repos_for_user(
        &self,
        user_id: Option<&Uuid>,
        path: Option<String>,
        timeout_ms: u64,
        hard_timeout_ms: u64,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        #[cfg(feature = "qa-mode")]
        {
            tracing::info!("QA mode: returning hardcoded QA repos instead of scanning filesystem");
            super::qa_repos::get_qa_repos()
        }

        #[cfg(not(feature = "qa-mode"))]
        {
            let base_path = match (user_id, path) {
                // User ID with custom path: validate path is within user workspace
                (Some(uid), Some(p)) => {
                    let requested_path = PathBuf::from(&p);
                    self.validate_path_for_user(uid, &requested_path)?
                }
                // User ID without custom path: use user's workspace directory
                (Some(uid), None) => Self::get_home_directory_for_user(Some(uid)),
                // No user ID with custom path: desktop mode, use the provided path
                (None, Some(p)) => PathBuf::from(p),
                // No user ID, no path: desktop mode, use system home directory
                (None, None) => Self::get_home_directory(),
            };

            Self::verify_directory(&base_path)?;
            self.list_git_repos_with_timeout(
                vec![base_path],
                timeout_ms,
                hard_timeout_ms,
                max_depth,
            )
            .await
        }
    }

    /// List common Git repository directories with user-aware path restrictions.
    ///
    /// In Kubernetes mode, restricts the search to the user's workspace directory.
    /// In Desktop mode (user_id is None), searches common development directories.
    ///
    /// # Arguments
    ///
    /// * `user_id` - Optional user UUID for workspace restriction
    /// * `timeout_ms` - Soft timeout in milliseconds
    /// * `hard_timeout_ms` - Hard timeout in milliseconds
    /// * `max_depth` - Maximum directory depth to search
    #[cfg_attr(feature = "qa-mode", allow(unused_variables))]
    pub async fn list_common_git_repos_for_user(
        &self,
        user_id: Option<&Uuid>,
        timeout_ms: u64,
        hard_timeout_ms: u64,
        max_depth: Option<usize>,
    ) -> Result<Vec<DirectoryEntry>, FilesystemError> {
        #[cfg(feature = "qa-mode")]
        {
            tracing::info!(
                "QA mode: returning hardcoded QA repos instead of scanning common directories"
            );
            super::qa_repos::get_qa_repos()
        }

        #[cfg(not(feature = "qa-mode"))]
        {
            match user_id {
                // Kubernetes mode: search within user's workspace only
                Some(uid) => {
                    let user_home = Self::get_home_directory_for_user(Some(uid));
                    if !user_home.exists() || !user_home.is_dir() {
                        // User workspace doesn't exist yet, return empty
                        return Ok(vec![]);
                    }

                    // Search common subdirectories within user's workspace
                    let search_strings = ["repos", "dev", "work", "code", "projects"];
                    let mut paths: Vec<PathBuf> = search_strings
                        .iter()
                        .map(|s| user_home.join(s))
                        .filter(|p| p.exists() && p.is_dir())
                        .collect();
                    paths.insert(0, user_home);

                    self.list_git_repos_with_timeout(paths, timeout_ms, hard_timeout_ms, max_depth)
                        .await
                }
                // Desktop mode: use existing behavior
                None => {
                    self.list_common_git_repos(timeout_ms, hard_timeout_ms, max_depth)
                        .await
                }
            }
        }
    }

    /// List directory contents with user-aware path validation.
    ///
    /// In Kubernetes mode, validates that the requested path is within the user's
    /// workspace boundary. In Desktop mode, uses the existing behavior.
    ///
    /// # Arguments
    ///
    /// * `user_id` - Optional user UUID for workspace restriction
    /// * `path` - Optional path to list (defaults to user's home directory)
    pub async fn list_directory_for_user(
        &self,
        user_id: Option<&Uuid>,
        path: Option<String>,
    ) -> Result<DirectoryListResponse, FilesystemError> {
        let resolved_path = match (user_id, path) {
            // User ID with custom path: validate path is within user workspace
            (Some(uid), Some(p)) => {
                let requested_path = PathBuf::from(&p);
                self.validate_path_for_user(uid, &requested_path)?
            }
            // User ID without custom path: use user's workspace directory
            (Some(uid), None) => Self::get_home_directory_for_user(Some(uid)),
            // No user ID with custom path: desktop mode, use the provided path
            (None, Some(p)) => PathBuf::from(p),
            // No user ID, no path: desktop mode, use system home directory
            (None, None) => Self::get_home_directory(),
        };

        Self::verify_directory(&resolved_path)?;

        let entries = fs::read_dir(&resolved_path)?;
        let mut directory_entries = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let metadata = entry.metadata().ok();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Skip hidden files/directories
                if name.starts_with('.') && name != ".." {
                    continue;
                }

                let is_directory = metadata.is_some_and(|m| m.is_dir());
                let is_git_repo = if is_directory {
                    path.join(".git").exists()
                } else {
                    false
                };

                directory_entries.push(DirectoryEntry {
                    name: name.to_string(),
                    path,
                    is_directory,
                    is_git_repo,
                    last_modified: None,
                });
            }
        }

        // Sort: directories first, then files, both alphabetically
        directory_entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Ok(DirectoryListResponse {
            entries: directory_entries,
            current_path: resolved_path.to_string_lossy().to_string(),
        })
    }
}
