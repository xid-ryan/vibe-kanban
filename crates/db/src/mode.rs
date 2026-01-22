//! Deployment mode detection for vibe-kanban.
//!
//! This module provides deployment mode detection to distinguish between
//! single-user desktop deployments (SQLite) and multi-user Kubernetes
//! deployments (PostgreSQL).

use std::env;

/// Environment variable name for deployment mode override.
const DEPLOYMENT_MODE_ENV: &str = "DEPLOYMENT_MODE";

/// Environment variable name for database URL.
const DATABASE_URL_ENV: &str = "DATABASE_URL";

/// Deployment mode for vibe-kanban application.
///
/// The deployment mode determines which database backend and features are enabled:
///
/// - `Desktop`: Single-user mode with SQLite database. This is the default mode
///   used when running the application locally as a desktop application.
///
/// - `Kubernetes`: Multi-user mode with PostgreSQL database. This mode enables
///   user isolation, shared storage, and horizontal scaling features required
///   for multi-tenant deployments.
///
/// # Detection Logic
///
/// The deployment mode is detected in the following order:
///
/// 1. Check `DEPLOYMENT_MODE` environment variable (values: "desktop", "kubernetes")
/// 2. Check if `DATABASE_URL` starts with "postgres" (indicates Kubernetes mode)
/// 3. Default to `Desktop` mode
///
/// # Example
///
/// ```
/// use db::DeploymentMode;
///
/// let mode = DeploymentMode::detect();
///
/// match mode {
///     DeploymentMode::Desktop => {
///         // Initialize SQLite database
///         // Use file-based configuration
///     }
///     DeploymentMode::Kubernetes => {
///         // Initialize PostgreSQL database
///         // Use database-backed configuration
///         // Enable user isolation features
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeploymentMode {
    /// Single-user desktop mode with SQLite database.
    ///
    /// In this mode:
    /// - SQLite is used for data storage
    /// - No user authentication is required
    /// - Configuration is stored in local files
    /// - Workspace isolation is not enforced
    #[default]
    Desktop,

    /// Multi-user Kubernetes mode with PostgreSQL database.
    ///
    /// In this mode:
    /// - PostgreSQL is used for data storage
    /// - JWT authentication is required
    /// - All database queries include user_id filtering
    /// - Configuration is stored in the database
    /// - Workspace paths are isolated per user
    Kubernetes,
}

impl DeploymentMode {
    /// Detect the deployment mode from environment variables.
    ///
    /// Detection priority:
    /// 1. `DEPLOYMENT_MODE` environment variable ("desktop" or "kubernetes")
    /// 2. `DATABASE_URL` starting with "postgres" indicates Kubernetes mode
    /// 3. Default to Desktop mode
    ///
    /// # Returns
    ///
    /// The detected deployment mode.
    ///
    /// # Example
    ///
    /// ```
    /// use db::DeploymentMode;
    ///
    /// // With DEPLOYMENT_MODE=kubernetes
    /// // std::env::set_var("DEPLOYMENT_MODE", "kubernetes");
    /// // let mode = DeploymentMode::detect();
    /// // assert_eq!(mode, DeploymentMode::Kubernetes);
    /// ```
    pub fn detect() -> Self {
        // First, check explicit DEPLOYMENT_MODE environment variable
        if let Ok(mode_str) = env::var(DEPLOYMENT_MODE_ENV) {
            let mode = mode_str.to_lowercase();
            match mode.as_str() {
                "kubernetes" | "k8s" => {
                    tracing::info!(
                        mode = "kubernetes",
                        source = "DEPLOYMENT_MODE env var",
                        "Detected deployment mode"
                    );
                    return Self::Kubernetes;
                }
                "desktop" | "local" => {
                    tracing::info!(
                        mode = "desktop",
                        source = "DEPLOYMENT_MODE env var",
                        "Detected deployment mode"
                    );
                    return Self::Desktop;
                }
                other => {
                    tracing::warn!(
                        value = other,
                        "Unknown DEPLOYMENT_MODE value, checking DATABASE_URL"
                    );
                }
            }
        }

        // Second, check if DATABASE_URL starts with "postgres"
        if let Ok(database_url) = env::var(DATABASE_URL_ENV) {
            if database_url.starts_with("postgres://") || database_url.starts_with("postgresql://")
            {
                tracing::info!(
                    mode = "kubernetes",
                    source = "DATABASE_URL prefix",
                    "Detected deployment mode from PostgreSQL URL"
                );
                return Self::Kubernetes;
            }
        }

        // Default to desktop mode
        tracing::info!(
            mode = "desktop",
            source = "default",
            "Using default deployment mode"
        );
        Self::Desktop
    }

    /// Check if the application is running in multi-user mode.
    ///
    /// Multi-user mode is enabled when running in Kubernetes mode.
    /// In this mode, all database queries must include user_id filtering
    /// and workspace paths must be validated against user boundaries.
    ///
    /// # Returns
    ///
    /// `true` if running in multi-user (Kubernetes) mode, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use db::DeploymentMode;
    ///
    /// let mode = DeploymentMode::Kubernetes;
    /// assert!(mode.is_multi_user());
    ///
    /// let mode = DeploymentMode::Desktop;
    /// assert!(!mode.is_multi_user());
    /// ```
    #[inline]
    pub fn is_multi_user(&self) -> bool {
        matches!(self, Self::Kubernetes)
    }

    /// Check if the application is running in single-user (desktop) mode.
    ///
    /// # Returns
    ///
    /// `true` if running in desktop mode, `false` otherwise.
    #[inline]
    pub fn is_desktop(&self) -> bool {
        matches!(self, Self::Desktop)
    }

    /// Check if the application is running in Kubernetes mode.
    ///
    /// # Returns
    ///
    /// `true` if running in Kubernetes mode, `false` otherwise.
    #[inline]
    pub fn is_kubernetes(&self) -> bool {
        matches!(self, Self::Kubernetes)
    }

    /// Get the expected database type for this deployment mode.
    ///
    /// # Returns
    ///
    /// A string describing the expected database type.
    pub fn expected_database(&self) -> &'static str {
        match self {
            Self::Desktop => "SQLite",
            Self::Kubernetes => "PostgreSQL",
        }
    }

    /// Get the mode as a string for logging and display.
    ///
    /// # Returns
    ///
    /// A lowercase string representation of the mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Kubernetes => "kubernetes",
        }
    }
}

impl std::fmt::Display for DeploymentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SAFETY: These tests manipulate environment variables.
    // Run with: cargo test -p db -- --test-threads=1

    unsafe fn set_env(key: &str, value: &str) {
        unsafe { env::set_var(key, value) };
    }

    unsafe fn remove_env(key: &str) {
        unsafe { env::remove_var(key) };
    }

    #[test]
    fn test_deployment_mode_default() {
        // SAFETY: Test environment
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
            remove_env(DATABASE_URL_ENV);
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Desktop);
        assert!(mode.is_desktop());
        assert!(!mode.is_kubernetes());
        assert!(!mode.is_multi_user());
    }

    #[test]
    fn test_deployment_mode_explicit_kubernetes() {
        // SAFETY: Test environment
        unsafe {
            set_env(DEPLOYMENT_MODE_ENV, "kubernetes");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Kubernetes);
        assert!(mode.is_kubernetes());
        assert!(mode.is_multi_user());

        // Clean up
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_explicit_k8s_shorthand() {
        // SAFETY: Test environment
        unsafe {
            set_env(DEPLOYMENT_MODE_ENV, "k8s");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Kubernetes);

        // Clean up
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_explicit_desktop() {
        // SAFETY: Test environment
        unsafe {
            set_env(DEPLOYMENT_MODE_ENV, "desktop");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Desktop);

        // Clean up
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_postgres_url() {
        // SAFETY: Test environment
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
            set_env(DATABASE_URL_ENV, "postgres://user:pass@localhost/db");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Kubernetes);

        // Clean up
        unsafe {
            remove_env(DATABASE_URL_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_postgresql_url() {
        // SAFETY: Test environment
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
            set_env(DATABASE_URL_ENV, "postgresql://user:pass@localhost/db");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Kubernetes);

        // Clean up
        unsafe {
            remove_env(DATABASE_URL_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_sqlite_url() {
        // SAFETY: Test environment
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
            set_env(DATABASE_URL_ENV, "sqlite://./data.db");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Desktop);

        // Clean up
        unsafe {
            remove_env(DATABASE_URL_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_explicit_overrides_url() {
        // SAFETY: Test environment
        unsafe {
            set_env(DEPLOYMENT_MODE_ENV, "desktop");
            set_env(DATABASE_URL_ENV, "postgres://user:pass@localhost/db");
        }

        // Explicit mode should take precedence over DATABASE_URL detection
        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Desktop);

        // Clean up
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
            remove_env(DATABASE_URL_ENV);
        }
    }

    #[test]
    fn test_deployment_mode_display() {
        assert_eq!(DeploymentMode::Desktop.to_string(), "desktop");
        assert_eq!(DeploymentMode::Kubernetes.to_string(), "kubernetes");
    }

    #[test]
    fn test_deployment_mode_as_str() {
        assert_eq!(DeploymentMode::Desktop.as_str(), "desktop");
        assert_eq!(DeploymentMode::Kubernetes.as_str(), "kubernetes");
    }

    #[test]
    fn test_expected_database() {
        assert_eq!(DeploymentMode::Desktop.expected_database(), "SQLite");
        assert_eq!(DeploymentMode::Kubernetes.expected_database(), "PostgreSQL");
    }

    #[test]
    fn test_deployment_mode_case_insensitive() {
        // SAFETY: Test environment
        unsafe {
            set_env(DEPLOYMENT_MODE_ENV, "KUBERNETES");
        }

        let mode = DeploymentMode::detect();
        assert_eq!(mode, DeploymentMode::Kubernetes);

        // Clean up
        unsafe {
            remove_env(DEPLOYMENT_MODE_ENV);
        }
    }
}
