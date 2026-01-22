use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use chrono::{DateTime, Utc};
use db::DeploymentMode;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use services::services::workspace_manager::WorkspaceManager;
use thiserror::Error;
use tokio::sync::mpsc;
use utils::shell::get_interactive_shell;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PtyError {
    #[error("Failed to create PTY: {0}")]
    CreateFailed(String),
    #[error("Session not found: {0}")]
    SessionNotFound(Uuid),
    #[error("Failed to write to PTY: {0}")]
    WriteFailed(String),
    #[error("Failed to resize PTY: {0}")]
    ResizeFailed(String),
    #[error("Session already closed")]
    SessionClosed,
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
}

/// PTY session with user ownership tracking for multi-user support.
struct PtySession {
    /// The user who owns this session
    user_id: Uuid,
    writer: Box<dyn Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    _output_handle: thread::JoinHandle<()>,
    closed: bool,
    /// Timestamp when the session was created
    created_at: DateTime<Utc>,
    /// Timestamp of last activity (write, resize, etc.)
    last_activity_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct PtyService {
    sessions: Arc<Mutex<HashMap<Uuid, PtySession>>>,
}

impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new PTY session for a user.
    ///
    /// In Kubernetes (multi-user) mode, validates that the working directory is within
    /// the user's workspace boundary and sets HOME to the user's workspace.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user creating the session
    /// * `working_dir` - The directory where the PTY session should start
    /// * `cols` - Number of columns for the terminal
    /// * `rows` - Number of rows for the terminal
    ///
    /// # Returns
    ///
    /// A tuple of (session_id, output_receiver) on success.
    pub async fn create_session(
        &self,
        user_id: Uuid,
        working_dir: PathBuf,
        cols: u16,
        rows: u16,
    ) -> Result<(Uuid, mpsc::UnboundedReceiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4();
        let (output_tx, output_rx) = mpsc::unbounded_channel();
        let shell = get_interactive_shell().await;

        // Validate working directory is within user's workspace (K8s mode)
        let validated_working_dir = WorkspaceManager::validate_user_path(&user_id, &working_dir)
            .map_err(|e| PtyError::Unauthorized(e.to_string()))?;

        // Get user's workspace home directory for setting HOME env var
        let user_home = WorkspaceManager::get_workspace_base_dir_for_user(&user_id);
        let mode = DeploymentMode::detect();

        let result = tokio::task::spawn_blocking(move || {
            let pty_system = NativePtySystem::default();

            let pty_pair = pty_system
                .openpty(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

            let mut cmd = CommandBuilder::new(&shell);
            cmd.cwd(&validated_working_dir);

            // Configure shell-specific options
            let shell_name = shell.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if shell_name == "powershell.exe" || shell_name == "pwsh.exe" {
                // PowerShell: use -NoLogo for cleaner startup
                cmd.arg("-NoLogo");
            } else if shell_name == "cmd.exe" {
                // cmd.exe: no special args needed
            } else {
                // Unix shells (bash, zsh, etc.): skip loading rc files
                cmd.arg("-f");
                cmd.env("PS1", "$ "); // Bash prompt
                cmd.env("PROMPT", "$ "); // Zsh prompt
            }

            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");

            // In K8s mode, set HOME to user's workspace directory
            if mode.is_kubernetes() {
                cmd.env("HOME", user_home.to_string_lossy().to_string());
            }

            let child = pty_pair
                .slave
                .spawn_command(cmd)
                .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

            let writer = pty_pair
                .master
                .take_writer()
                .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

            let mut reader = pty_pair
                .master
                .try_clone_reader()
                .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

            let output_handle = thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if output_tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                drop(child);
            });

            Ok::<_, PtyError>((pty_pair.master, writer, output_handle))
        })
        .await
        .map_err(|e| PtyError::CreateFailed(e.to_string()))??;

        let (master, writer, output_handle) = result;

        let now = Utc::now();
        let session = PtySession {
            user_id,
            writer,
            master,
            _output_handle: output_handle,
            closed: false,
            created_at: now,
            last_activity_at: now,
        };

        self.sessions
            .lock()
            .map_err(|e| PtyError::CreateFailed(e.to_string()))?
            .insert(session_id, session);

        tracing::info!(
            session_id = %session_id,
            user_id = %user_id,
            "Created PTY session"
        );

        Ok((session_id, output_rx))
    }

    /// Validate that a session belongs to the specified user.
    ///
    /// Returns `PtyError::SessionNotFound` if the session doesn't exist or
    /// belongs to a different user (to avoid leaking information about
    /// other users' sessions).
    fn validate_session_ownership(&self, session_id: &Uuid, user_id: &Uuid) -> Result<(), PtyError> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| PtyError::CreateFailed(e.to_string()))?;

        match sessions.get(session_id) {
            Some(session) if session.user_id == *user_id => Ok(()),
            Some(_) => {
                // Session exists but belongs to different user - return NotFound
                // to avoid leaking information about other users' sessions
                let timestamp = Utc::now().to_rfc3339();
                tracing::warn!(
                    action = "unauthorized_access_attempt",
                    session_id = %session_id,
                    requesting_user = %user_id,
                    resource_type = "pty_session",
                    timestamp = %timestamp,
                    security_event = true,
                    "Unauthorized PTY session access attempt"
                );
                Err(PtyError::SessionNotFound(*session_id))
            }
            None => Err(PtyError::SessionNotFound(*session_id)),
        }
    }

    /// Write data to a PTY session.
    ///
    /// Validates that the session belongs to the specified user before writing.
    pub async fn write(
        &self,
        user_id: Uuid,
        session_id: Uuid,
        data: &[u8],
    ) -> Result<(), PtyError> {
        // First validate ownership (uses shared lock)
        self.validate_session_ownership(&session_id, &user_id)?;

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| PtyError::WriteFailed(e.to_string()))?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or(PtyError::SessionNotFound(session_id))?;

        if session.closed {
            return Err(PtyError::SessionClosed);
        }

        session
            .writer
            .write_all(data)
            .map_err(|e| PtyError::WriteFailed(e.to_string()))?;

        session
            .writer
            .flush()
            .map_err(|e| PtyError::WriteFailed(e.to_string()))?;

        // Update activity timestamp
        session.last_activity_at = Utc::now();

        Ok(())
    }

    /// Resize a PTY session.
    ///
    /// Validates that the session belongs to the specified user before resizing.
    pub async fn resize(
        &self,
        user_id: Uuid,
        session_id: Uuid,
        cols: u16,
        rows: u16,
    ) -> Result<(), PtyError> {
        // First validate ownership
        self.validate_session_ownership(&session_id, &user_id)?;

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| PtyError::ResizeFailed(e.to_string()))?;
        let session = sessions
            .get_mut(&session_id)
            .ok_or(PtyError::SessionNotFound(session_id))?;

        if session.closed {
            return Err(PtyError::SessionClosed);
        }

        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::ResizeFailed(e.to_string()))?;

        // Update activity timestamp
        session.last_activity_at = Utc::now();

        Ok(())
    }

    /// Close a PTY session.
    ///
    /// Validates that the session belongs to the specified user before closing.
    pub async fn close_session(&self, user_id: Uuid, session_id: Uuid) -> Result<(), PtyError> {
        // Validate ownership first
        self.validate_session_ownership(&session_id, &user_id)?;

        if let Some(mut session) = self
            .sessions
            .lock()
            .map_err(|_| PtyError::SessionClosed)?
            .remove(&session_id)
        {
            session.closed = true;
            tracing::info!(
                session_id = %session_id,
                user_id = %user_id,
                "Closed PTY session"
            );
        }
        Ok(())
    }

    /// Check if a session exists (without user validation).
    ///
    /// Note: This method does not validate user ownership. For multi-user
    /// scenarios, use session operations that require user_id.
    pub fn session_exists(&self, session_id: &Uuid) -> bool {
        self.sessions
            .lock()
            .map(|s| s.contains_key(session_id))
            .unwrap_or(false)
    }

    /// Check if a session exists and belongs to the specified user.
    pub fn session_exists_for_user(&self, session_id: &Uuid, user_id: &Uuid) -> bool {
        self.sessions
            .lock()
            .map(|sessions| {
                sessions
                    .get(session_id)
                    .is_some_and(|session| session.user_id == *user_id)
            })
            .unwrap_or(false)
    }

    /// List all session IDs belonging to a specific user.
    pub fn list_user_sessions(&self, user_id: &Uuid) -> Vec<Uuid> {
        self.sessions
            .lock()
            .map(|sessions| {
                sessions
                    .iter()
                    .filter(|(_, session)| session.user_id == *user_id)
                    .map(|(id, _)| *id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clean up idle sessions that have been inactive for longer than the specified timeout.
    ///
    /// Returns the number of sessions cleaned up.
    pub fn cleanup_idle_sessions(&self, timeout: Duration) -> usize {
        let now = Utc::now();
        let timeout_chrono = chrono::Duration::from_std(timeout).unwrap_or(chrono::Duration::minutes(30));

        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let expired_sessions: Vec<Uuid> = sessions
            .iter()
            .filter(|(_, session)| {
                let idle_duration = now.signed_duration_since(session.last_activity_at);
                idle_duration > timeout_chrono
            })
            .map(|(id, _)| *id)
            .collect();

        let count = expired_sessions.len();
        for session_id in &expired_sessions {
            if let Some(mut session) = sessions.remove(session_id) {
                session.closed = true;
                tracing::info!(
                    session_id = %session_id,
                    user_id = %session.user_id,
                    idle_since = %session.last_activity_at,
                    "Cleaned up idle PTY session"
                );
            }
        }

        count
    }

    /// Close all sessions belonging to a specific user.
    ///
    /// Returns the number of sessions closed.
    pub fn close_all_user_sessions(&self, user_id: &Uuid) -> usize {
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let user_sessions: Vec<Uuid> = sessions
            .iter()
            .filter(|(_, session)| session.user_id == *user_id)
            .map(|(id, _)| *id)
            .collect();

        let count = user_sessions.len();
        for session_id in &user_sessions {
            if let Some(mut session) = sessions.remove(session_id) {
                session.closed = true;
                tracing::info!(
                    session_id = %session_id,
                    user_id = %user_id,
                    "Closed user PTY session during cleanup"
                );
            }
        }

        count
    }
}

impl Default for PtyService {
    fn default() -> Self {
        Self::new()
    }
}

/// Cleanup job for PTY sessions.
///
/// This module provides a background task that periodically cleans up
/// idle PTY sessions to prevent resource leaks and maintain security
/// in multi-user environments.
pub mod cleanup {
    use super::*;
    use std::time::Duration;

    /// Default cleanup interval (5 minutes).
    pub const DEFAULT_CLEANUP_INTERVAL_SECS: u64 = 300;

    /// Default session idle timeout (30 minutes).
    pub const DEFAULT_SESSION_TIMEOUT_SECS: u64 = 1800;

    /// Spawns a background task that periodically cleans up idle PTY sessions.
    ///
    /// This task runs every `cleanup_interval` and removes sessions that have
    /// been idle for longer than `session_timeout`. It logs cleanup actions
    /// with user_id and session_id for audit purposes.
    ///
    /// # Arguments
    ///
    /// * `pty_service` - The PTY service instance to clean up.
    /// * `cleanup_interval` - How often to run the cleanup (default: 5 minutes).
    /// * `session_timeout` - How long a session can be idle before cleanup (default: 30 minutes).
    ///
    /// # Returns
    ///
    /// A `JoinHandle` that can be used to monitor or cancel the cleanup task.
    pub fn spawn_cleanup_job(
        pty_service: PtyService,
        cleanup_interval: Option<Duration>,
        session_timeout: Option<Duration>,
    ) -> tokio::task::JoinHandle<()> {
        let interval = cleanup_interval.unwrap_or(Duration::from_secs(DEFAULT_CLEANUP_INTERVAL_SECS));
        let timeout = session_timeout.unwrap_or(Duration::from_secs(DEFAULT_SESSION_TIMEOUT_SECS));

        tracing::info!(
            cleanup_interval_secs = interval.as_secs(),
            session_timeout_secs = timeout.as_secs(),
            "Starting PTY session cleanup job"
        );

        tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(interval);

            loop {
                cleanup_interval.tick().await;

                tracing::debug!("Running PTY session cleanup...");

                let cleaned_count = pty_service.cleanup_idle_sessions(timeout);

                if cleaned_count > 0 {
                    tracing::info!(
                        cleaned_sessions = cleaned_count,
                        action = "pty_session_cleanup",
                        "Cleaned up idle PTY sessions"
                    );
                } else {
                    tracing::debug!("No idle PTY sessions to clean up");
                }
            }
        })
    }

    /// Get cleanup configuration from environment variables.
    ///
    /// Environment variables:
    /// - `PTY_CLEANUP_INTERVAL_SECS`: Cleanup interval in seconds (default: 300)
    /// - `PTY_SESSION_TIMEOUT_SECS`: Session timeout in seconds (default: 1800)
    ///
    /// # Returns
    ///
    /// A tuple of (cleanup_interval, session_timeout) as `Duration` values.
    pub fn get_cleanup_config_from_env() -> (Duration, Duration) {
        let interval_secs: u64 = std::env::var("PTY_CLEANUP_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CLEANUP_INTERVAL_SECS);

        let timeout_secs: u64 = std::env::var("PTY_SESSION_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_SESSION_TIMEOUT_SECS);

        (
            Duration::from_secs(interval_secs),
            Duration::from_secs(timeout_secs),
        )
    }
}
