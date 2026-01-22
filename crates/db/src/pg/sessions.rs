//! PostgreSQL queries for sessions with user_id filtering.
//!
//! This module provides PostgreSQL-specific query functions for the sessions table
//! that include user_id filtering for multi-tenant isolation in Kubernetes deployments.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::session::{CreateSession, Session};

/// Find a session by ID, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Session ID to find
///
/// # Returns
///
/// The session if found and owned by the user, None otherwise.
pub async fn find_by_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Session>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            id,
            workspace_id,
            executor,
            created_at,
            updated_at
        FROM sessions
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| Session {
        id: r.id,
        workspace_id: r.workspace_id,
        executor: r.executor,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Find all sessions for a workspace, ensuring they belong to the specified user.
/// Ordered by most recently used (most recent non-dev server execution process).
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to find sessions for
///
/// # Returns
///
/// A vector of sessions.
pub async fn find_by_workspace_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<Session>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT s.id, s.workspace_id, s.executor, s.created_at, s.updated_at
        FROM sessions s
        LEFT JOIN (
            SELECT ep.session_id, MAX(ep.created_at) as last_used
            FROM execution_processes ep
            WHERE ep.run_reason != 'devserver' AND ep.dropped = FALSE
            GROUP BY ep.session_id
        ) latest_ep ON s.id = latest_ep.session_id
        WHERE s.workspace_id = $1 AND s.user_id = $2
        ORDER BY COALESCE(latest_ep.last_used, s.created_at) DESC"#,
        workspace_id,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| Session {
            id: r.id,
            workspace_id: r.workspace_id,
            executor: r.executor,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Find the most recently used session for a workspace, ensuring it belongs to the specified user.
/// "Most recently used" is defined as the most recent non-dev server execution process.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to find session for
///
/// # Returns
///
/// The most recently used session, or None if no sessions exist.
pub async fn find_latest_by_workspace_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Option<Session>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT s.id, s.workspace_id, s.executor, s.created_at, s.updated_at
        FROM sessions s
        LEFT JOIN (
            SELECT ep.session_id, MAX(ep.created_at) as last_used
            FROM execution_processes ep
            WHERE ep.run_reason != 'devserver' AND ep.dropped = FALSE
            GROUP BY ep.session_id
        ) latest_ep ON s.id = latest_ep.session_id
        WHERE s.workspace_id = $1 AND s.user_id = $2
        ORDER BY COALESCE(latest_ep.last_used, s.created_at) DESC
        LIMIT 1"#,
        workspace_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| Session {
        id: r.id,
        workspace_id: r.workspace_id,
        executor: r.executor,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new session for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID who owns this session
/// * `data` - Session creation data
/// * `id` - Pre-generated UUID for the session
/// * `workspace_id` - Workspace ID this session belongs to
///
/// # Returns
///
/// The created session.
pub async fn create_for_user(
    pool: &PgPool,
    user_id: Uuid,
    data: &CreateSession,
    id: Uuid,
    workspace_id: Uuid,
) -> Result<Session, sqlx::Error> {
    let record = sqlx::query!(
        r#"INSERT INTO sessions (id, user_id, workspace_id, executor)
        VALUES ($1, $2, $3, $4)
        RETURNING
            id,
            workspace_id,
            executor,
            created_at,
            updated_at"#,
        id,
        user_id,
        workspace_id,
        data.executor
    )
    .fetch_one(pool)
    .await?;

    Ok(Session {
        id: record.id,
        workspace_id: record.workspace_id,
        executor: record.executor,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Delete a session, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Session ID to delete
///
/// # Returns
///
/// The number of rows deleted (0 or 1).
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM sessions WHERE id = $1 AND user_id = $2",
        id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    // Integration tests would go here, requiring a running PostgreSQL instance
    // and are marked with #[ignore] to not run in normal test suites
}
