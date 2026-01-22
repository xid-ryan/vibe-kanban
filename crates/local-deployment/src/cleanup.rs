//! Resource cleanup module for the local deployment.
//!
//! This module provides background cleanup tasks for managing resources
//! in both desktop and multi-user Kubernetes deployments. It handles:
//!
//! - PTY session cleanup (idle sessions)
//! - Orphaned process cleanup (processes without active sessions)
//! - Workspace cleanup (expired workspaces)
//!
//! All cleanup actions are logged with structured fields for audit purposes.

use std::time::Duration;

use crate::container::LocalContainerService;
use crate::pty::PtyService;

/// Default cleanup interval for the combined cleanup job (5 minutes).
const DEFAULT_CLEANUP_INTERVAL_SECS: u64 = 300;

/// Cleanup job configuration.
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// How often to run the cleanup job.
    pub cleanup_interval: Duration,
    /// PTY session idle timeout.
    pub pty_session_timeout: Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            cleanup_interval: Duration::from_secs(DEFAULT_CLEANUP_INTERVAL_SECS),
            pty_session_timeout: Duration::from_secs(
                crate::pty::cleanup::DEFAULT_SESSION_TIMEOUT_SECS,
            ),
        }
    }
}

impl CleanupConfig {
    /// Load cleanup configuration from environment variables.
    ///
    /// Environment variables:
    /// - `CLEANUP_INTERVAL_SECS`: Combined cleanup interval (default: 300)
    /// - `PTY_SESSION_TIMEOUT_SECS`: PTY session timeout (default: 1800)
    pub fn from_env() -> Self {
        let cleanup_interval_secs: u64 = std::env::var("CLEANUP_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_CLEANUP_INTERVAL_SECS);

        let (_, pty_timeout) = crate::pty::cleanup::get_cleanup_config_from_env();

        Self {
            cleanup_interval: Duration::from_secs(cleanup_interval_secs),
            pty_session_timeout: pty_timeout,
        }
    }
}

/// Spawns the combined resource cleanup job.
///
/// This job runs periodically and cleans up:
/// - Idle PTY sessions
/// - Orphaned execution processes
///
/// All cleanup actions are logged with structured fields (user_id, session_id,
/// execution_id, action type, timestamp) for security auditing.
///
/// # Arguments
///
/// * `pty_service` - The PTY service to clean up idle sessions.
/// * `container_service` - The container service to clean up orphaned processes.
/// * `config` - Cleanup job configuration.
///
/// # Returns
///
/// A `JoinHandle` that can be used to monitor or cancel the cleanup task.
pub fn spawn_cleanup_job(
    pty_service: PtyService,
    container_service: LocalContainerService,
    config: CleanupConfig,
) -> tokio::task::JoinHandle<()> {
    tracing::info!(
        cleanup_interval_secs = config.cleanup_interval.as_secs(),
        pty_session_timeout_secs = config.pty_session_timeout.as_secs(),
        action = "cleanup_job_started",
        "Starting combined resource cleanup job"
    );

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(config.cleanup_interval);

        loop {
            interval.tick().await;

            let timestamp = chrono::Utc::now().to_rfc3339();
            tracing::debug!(
                timestamp = %timestamp,
                action = "cleanup_cycle_started",
                "Running resource cleanup cycle"
            );

            // 1. Clean up idle PTY sessions
            let pty_cleaned = pty_service.cleanup_idle_sessions(config.pty_session_timeout);
            if pty_cleaned > 0 {
                tracing::info!(
                    cleaned_count = pty_cleaned,
                    action = "pty_session_cleanup",
                    resource_type = "pty_session",
                    timestamp = %timestamp,
                    "Cleaned up idle PTY sessions"
                );
            }

            // 2. Clean up orphaned execution processes
            let orphaned_cleaned = cleanup_orphaned_processes(&container_service).await;
            if orphaned_cleaned > 0 {
                tracing::info!(
                    cleaned_count = orphaned_cleaned,
                    action = "orphaned_process_cleanup",
                    resource_type = "execution_process",
                    timestamp = %timestamp,
                    "Cleaned up orphaned execution processes"
                );
            }

            tracing::debug!(
                pty_sessions_cleaned = pty_cleaned,
                processes_cleaned = orphaned_cleaned,
                action = "cleanup_cycle_completed",
                timestamp = %timestamp,
                "Resource cleanup cycle completed"
            );
        }
    })
}

/// Clean up orphaned execution processes.
///
/// An orphaned process is one that:
/// - Has no active child process handle in the child_store
/// - But still has ownership tracking in execution_owners
///
/// This can happen if the process exits abnormally without triggering
/// the normal cleanup in spawn_exit_monitor.
///
/// # Arguments
///
/// * `container_service` - The container service to clean up.
///
/// # Returns
///
/// The number of orphaned processes cleaned up.
async fn cleanup_orphaned_processes(container_service: &LocalContainerService) -> usize {
    let timestamp = chrono::Utc::now().to_rfc3339();

    // Get all tracked execution owners
    let all_processes = container_service.list_user_processes(None).await;

    let mut cleaned_count = 0;

    for (execution_id, ownership) in all_processes {
        // Check if this execution has an active child process
        let has_child = container_service.get_child_from_store(&execution_id).await.is_some();

        if !has_child {
            // No active child process - this is orphaned
            tracing::info!(
                execution_id = %execution_id,
                user_id = ?ownership.user_id,
                workspace_id = %ownership.workspace_id,
                action = "orphaned_process_cleanup",
                resource_type = "execution_process",
                timestamp = %timestamp,
                "Cleaning up orphaned execution process"
            );

            // Remove the orphaned ownership tracking
            container_service.remove_execution_owner(&execution_id).await;
            cleaned_count += 1;
        }
    }

    cleaned_count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_config_default() {
        let config = CleanupConfig::default();
        assert_eq!(config.cleanup_interval.as_secs(), DEFAULT_CLEANUP_INTERVAL_SECS);
        assert_eq!(
            config.pty_session_timeout.as_secs(),
            crate::pty::cleanup::DEFAULT_SESSION_TIMEOUT_SECS
        );
    }
}
