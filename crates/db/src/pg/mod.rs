//! PostgreSQL database service for multi-user Kubernetes deployments.
//!
//! This module provides PostgreSQL support for the vibe-kanban application when running
//! in Kubernetes multi-user mode. It supports:
//! - Connection pooling with configurable max_connections
//! - DATABASE_URL environment variable for connection strings
//! - Automatic migration execution on startup
//! - User-scoped queries for multi-tenant isolation
//!
//! Query submodules (projects, tasks, workspaces, sessions, repos) are only compiled
//! when the `postgres` feature is enabled, as they require SQLx compile-time query
//! validation against a PostgreSQL database schema.

use std::env;
use std::sync::Arc;

use sqlx::{
    Error,
    PgPool,
    Postgres,
    postgres::{PgConnectOptions, PgConnection, PgPoolOptions},
};

// Query submodules for multi-user PostgreSQL queries.
// These are only compiled when the `postgres` feature is enabled because
// SQLx query macros require compile-time validation against the database schema.
#[cfg(feature = "postgres")]
pub mod execution_processes;
#[cfg(feature = "postgres")]
pub mod projects;
#[cfg(feature = "postgres")]
pub mod repos;
#[cfg(feature = "postgres")]
pub mod sessions;
#[cfg(feature = "postgres")]
pub mod tasks;
#[cfg(feature = "postgres")]
pub mod workspaces;

/// Default maximum number of connections in the pool.
const DEFAULT_MAX_CONNECTIONS: u32 = 10;

/// Environment variable name for the database URL.
const DATABASE_URL_ENV: &str = "DATABASE_URL";

/// Environment variable name for max connections override.
const MAX_CONNECTIONS_ENV: &str = "DB_MAX_CONNECTIONS";

/// Run PostgreSQL migrations against the database.
///
/// This function runs all pending migrations from the ./pg_migrations directory.
/// PostgreSQL-specific migrations are separate from SQLite migrations to support
/// proper UUID types, TIMESTAMPTZ, JSONB, and user_id columns for multi-tenant isolation.
/// Migrations are expected to be idempotent and safe to run multiple times.
async fn run_pg_migrations(pool: &PgPool) -> Result<(), Error> {
    sqlx::migrate!("./pg_migrations")
        .run(pool)
        .await
        .map_err(|e| Error::Migrate(Box::new(e)))
}

/// PostgreSQL database service for multi-user deployments.
///
/// This service provides a connection pool to PostgreSQL and handles
/// migrations automatically on initialization.
///
/// # Example
///
/// ```ignore
/// // Set DATABASE_URL environment variable before creating service
/// std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/vibe_kanban");
///
/// let db_service = DBServicePg::new().await?;
/// let pool = &db_service.pool;
/// // Use pool for queries...
/// ```
#[derive(Clone)]
pub struct DBServicePg {
    /// The PostgreSQL connection pool.
    pub pool: PgPool,
}

impl DBServicePg {
    /// Create a new PostgreSQL database service.
    ///
    /// This function reads the `DATABASE_URL` environment variable to establish
    /// a connection to PostgreSQL. It also runs pending migrations automatically.
    ///
    /// # Environment Variables
    ///
    /// - `DATABASE_URL`: Required. PostgreSQL connection string.
    ///   Format: `postgres://user:password@host:port/database`
    /// - `DB_MAX_CONNECTIONS`: Optional. Maximum pool connections (default: 10).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `DATABASE_URL` environment variable is not set
    /// - Connection to the database fails
    /// - Migrations fail to run
    pub async fn new() -> Result<DBServicePg, Error> {
        let pool = Self::create_pool_simple().await?;
        Ok(DBServicePg { pool })
    }

    /// Create a new PostgreSQL database service with an after_connect hook.
    ///
    /// The hook function is called after each new connection is established,
    /// allowing for connection-level setup such as setting session variables.
    ///
    /// # Arguments
    ///
    /// * `after_connect` - A function called after each connection is established
    ///
    /// # Example
    ///
    /// ```ignore
    /// let db_service = DBServicePg::new_with_after_connect(|conn| {
    ///     Box::pin(async move {
    ///         // Set session-level configuration
    ///         sqlx::query("SET statement_timeout = '30s'")
    ///             .execute(conn)
    ///             .await?;
    ///         Ok(())
    ///     })
    /// }).await?;
    /// ```
    pub async fn new_with_after_connect<F>(after_connect: F) -> Result<DBServicePg, Error>
    where
        F: for<'a> Fn(
                &'a mut PgConnection,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>,
            > + Send
            + Sync
            + 'static,
    {
        let pool = Self::create_pool_with_hook(Arc::new(after_connect)).await?;
        Ok(DBServicePg { pool })
    }

    /// Create a simple PostgreSQL connection pool without hooks.
    ///
    /// # Returns
    ///
    /// A configured PostgreSQL connection pool with migrations applied.
    async fn create_pool_simple() -> Result<PgPool, Error> {
        let database_url = Self::get_database_url()?;
        let max_connections = Self::get_max_connections();

        tracing::info!(
            max_connections = max_connections,
            "Initializing PostgreSQL connection pool"
        );

        let options: PgConnectOptions = database_url
            .parse()
            .map_err(|_| Error::Configuration("Invalid DATABASE_URL format".into()))?;

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;

        // Run migrations
        run_pg_migrations(&pool).await?;

        tracing::info!("PostgreSQL connection pool initialized successfully");
        Ok(pool)
    }

    /// Create a PostgreSQL connection pool with an after_connect hook.
    ///
    /// # Arguments
    ///
    /// * `after_connect` - Hook function called after each connection
    ///
    /// # Returns
    ///
    /// A configured PostgreSQL connection pool with migrations applied.
    async fn create_pool_with_hook<F>(after_connect: Arc<F>) -> Result<PgPool, Error>
    where
        F: for<'a> Fn(
                &'a mut PgConnection,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), Error>> + Send + 'a>,
            > + Send
            + Sync
            + 'static,
    {
        let database_url = Self::get_database_url()?;
        let max_connections = Self::get_max_connections();

        tracing::info!(
            max_connections = max_connections,
            "Initializing PostgreSQL connection pool with after_connect hook"
        );

        let options: PgConnectOptions = database_url
            .parse()
            .map_err(|_| Error::Configuration("Invalid DATABASE_URL format".into()))?;

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .after_connect(move |conn, _meta| {
                let hook = after_connect.clone();
                Box::pin(async move {
                    hook(conn).await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;

        // Run migrations
        run_pg_migrations(&pool).await?;

        tracing::info!("PostgreSQL connection pool initialized successfully");
        Ok(pool)
    }

    /// Create a PostgreSQL connection pool from an explicit database URL.
    ///
    /// This is useful for testing or when you want to specify the URL directly
    /// rather than reading from environment variables.
    ///
    /// # Arguments
    ///
    /// * `database_url` - PostgreSQL connection string
    /// * `max_connections` - Maximum number of connections in the pool
    ///
    /// # Returns
    ///
    /// A configured PostgreSQL database service.
    pub async fn new_with_url(
        database_url: &str,
        max_connections: u32,
    ) -> Result<DBServicePg, Error> {
        tracing::info!(
            max_connections = max_connections,
            "Initializing PostgreSQL connection pool with explicit URL"
        );

        let options: PgConnectOptions = database_url
            .parse()
            .map_err(|_| Error::Configuration("Invalid DATABASE_URL format".into()))?;

        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await?;

        // Run migrations
        run_pg_migrations(&pool).await?;

        tracing::info!("PostgreSQL connection pool initialized successfully");
        Ok(DBServicePg { pool })
    }

    /// Get the database URL from environment variables.
    ///
    /// # Returns
    ///
    /// The DATABASE_URL environment variable value.
    ///
    /// # Errors
    ///
    /// Returns an error if DATABASE_URL is not set.
    fn get_database_url() -> Result<String, Error> {
        env::var(DATABASE_URL_ENV).map_err(|_| {
            Error::Configuration(
                format!(
                    "{} environment variable not set. Required for PostgreSQL mode.",
                    DATABASE_URL_ENV
                )
                .into(),
            )
        })
    }

    /// Get the maximum connections from environment or use default.
    ///
    /// Reads from `DB_MAX_CONNECTIONS` environment variable, falling back
    /// to `DEFAULT_MAX_CONNECTIONS` (10) if not set or invalid.
    fn get_max_connections() -> u32 {
        env::var(MAX_CONNECTIONS_ENV)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONNECTIONS)
    }

    /// Check if the database is reachable.
    ///
    /// Performs a simple query to verify connectivity.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the database is reachable, error otherwise.
    pub async fn health_check(&self) -> Result<(), Error> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
    }

    /// Get the current pool statistics.
    ///
    /// # Returns
    ///
    /// A tuple of (active_connections, idle_connections, max_connections).
    pub fn pool_stats(&self) -> (u32, u32, u32) {
        let size = self.pool.size();
        let idle = self.pool.num_idle() as u32;
        let max = self.pool.options().get_max_connections();
        (size - idle, idle, max)
    }
}

/// PostgreSQL transaction type alias for convenience.
pub type PgTx<'a> = sqlx::Transaction<'a, Postgres>;

#[cfg(test)]
mod tests {
    use super::*;

    // Helper functions for environment variable manipulation in tests
    // SAFETY: These call unsafe std::env functions. Callers must ensure
    // tests are run single-threaded (--test-threads=1) when using these.

    unsafe fn set_env(key: &str, value: &str) {
        // SAFETY: Caller ensures single-threaded test execution
        unsafe { env::set_var(key, value) };
    }

    unsafe fn remove_env(key: &str) {
        // SAFETY: Caller ensures single-threaded test execution
        unsafe { env::remove_var(key) };
    }

    // These tests modify environment variables and should be run with --test-threads=1
    // to avoid race conditions. For unit tests that don't depend on env vars,
    // we test the parsing logic directly.

    #[test]
    fn test_max_connections_parsing_default() {
        // Test that get_max_connections returns default when env var is not set
        // We test the parsing logic by temporarily checking the current value
        // Note: This test may be flaky if other tests set the env var concurrently
        // Run with: cargo test -p db -- --test-threads=1

        // SAFETY: Test environment
        unsafe { remove_env(MAX_CONNECTIONS_ENV) };
        let max = DBServicePg::get_max_connections();
        assert_eq!(max, DEFAULT_MAX_CONNECTIONS, "Expected default when env var not set");
    }

    #[test]
    fn test_max_connections_parsing_custom() {
        // SAFETY: Test environment
        unsafe { set_env(MAX_CONNECTIONS_ENV, "25") };
        let max = DBServicePg::get_max_connections();
        assert_eq!(max, 25, "Expected custom value from env var");
        // Clean up
        unsafe { remove_env(MAX_CONNECTIONS_ENV) };
    }

    #[test]
    fn test_max_connections_parsing_invalid() {
        // SAFETY: Test environment
        unsafe { set_env(MAX_CONNECTIONS_ENV, "not_a_number") };
        let max = DBServicePg::get_max_connections();
        assert_eq!(max, DEFAULT_MAX_CONNECTIONS, "Expected default for invalid env var");
        // Clean up
        unsafe { remove_env(MAX_CONNECTIONS_ENV) };
    }

    #[test]
    fn test_database_url_missing() {
        // SAFETY: Test environment
        unsafe { remove_env(DATABASE_URL_ENV) };
        let result = DBServicePg::get_database_url();
        assert!(result.is_err(), "Expected error when DATABASE_URL not set");
    }

    #[test]
    fn test_database_url_present() {
        let test_url = "postgres://test:test@localhost/test_db";
        // SAFETY: Test environment
        unsafe { set_env(DATABASE_URL_ENV, test_url) };
        let result = DBServicePg::get_database_url();
        assert!(result.is_ok(), "Expected Ok when DATABASE_URL is set");
        assert_eq!(result.unwrap(), test_url);
        // Clean up
        unsafe { remove_env(DATABASE_URL_ENV) };
    }

    // Test constant values
    #[test]
    fn test_default_constants() {
        assert_eq!(DEFAULT_MAX_CONNECTIONS, 10);
        assert_eq!(DATABASE_URL_ENV, "DATABASE_URL");
        assert_eq!(MAX_CONNECTIONS_ENV, "DB_MAX_CONNECTIONS");
    }

    // Integration tests that require a running PostgreSQL instance
    // These are marked with #[ignore] and can be run with `cargo test -- --ignored`

    #[tokio::test]
    #[ignore = "requires running PostgreSQL instance"]
    async fn test_pool_initialization() {
        // This test requires DATABASE_URL to be set to a valid PostgreSQL connection
        let result = DBServicePg::new().await;
        assert!(result.is_ok(), "Failed to initialize pool: {:?}", result.err());

        let service = result.unwrap();
        let (active, idle, max) = service.pool_stats();
        assert!(max >= DEFAULT_MAX_CONNECTIONS);
        assert!(active + idle <= max);
    }

    #[tokio::test]
    #[ignore = "requires running PostgreSQL instance"]
    async fn test_health_check() {
        let service = DBServicePg::new().await.expect("Failed to create service");
        let result = service.health_check().await;
        assert!(result.is_ok(), "Health check failed: {:?}", result.err());
    }

    #[tokio::test]
    #[ignore = "requires running PostgreSQL instance"]
    async fn test_new_with_explicit_url() {
        let database_url = env::var(DATABASE_URL_ENV)
            .expect("DATABASE_URL must be set for this test");

        let service = DBServicePg::new_with_url(&database_url, 5).await;
        assert!(service.is_ok(), "Failed to create service: {:?}", service.err());

        let service = service.unwrap();
        let (_, _, max) = service.pool_stats();
        assert_eq!(max, 5);
    }
}
