//! PostgreSQL queries for workspaces with user_id filtering.
//!
//! This module provides PostgreSQL-specific query functions for the workspaces table
//! that include user_id filtering for multi-tenant isolation in Kubernetes deployments.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::workspace::{CreateWorkspace, Workspace, WorkspaceWithStatus};

/// Find a workspace by ID, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Workspace ID to find
///
/// # Returns
///
/// The workspace if found and owned by the user, None otherwise.
pub async fn find_by_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Workspace>, sqlx::Error> {
    // Note: PostgreSQL schema doesn't have setup_completed_at column
    // We return None for that field to maintain compatibility with the Workspace struct
    let record = sqlx::query!(
        r#"SELECT
            id,
            task_id,
            container_ref,
            branch,
            agent_working_dir,
            created_at,
            updated_at,
            archived,
            pinned,
            name
        FROM workspaces
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| Workspace {
        id: r.id,
        task_id: r.task_id,
        container_ref: r.container_ref,
        branch: r.branch,
        agent_working_dir: r.agent_working_dir,
        setup_completed_at: None,
        created_at: r.created_at,
        updated_at: r.updated_at,
        archived: r.archived,
        pinned: r.pinned,
        name: r.name,
    }))
}

/// Fetch all workspaces for a user, optionally filtered by task_id. Newest first.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `task_id` - Optional task ID to filter by
///
/// # Returns
///
/// A vector of workspaces.
pub async fn fetch_all_for_user(
    pool: &PgPool,
    user_id: Uuid,
    task_id: Option<Uuid>,
) -> Result<Vec<Workspace>, sqlx::Error> {
    let workspaces = match task_id {
        Some(tid) => {
            let records = sqlx::query!(
                r#"SELECT
                    id,
                    task_id,
                    container_ref,
                    branch,
                    agent_working_dir,
                    created_at,
                    updated_at,
                    archived,
                    pinned,
                    name
                FROM workspaces
                WHERE task_id = $1 AND user_id = $2
                ORDER BY created_at DESC"#,
                tid,
                user_id
            )
            .fetch_all(pool)
            .await?;

            records
                .into_iter()
                .map(|r| Workspace {
                    id: r.id,
                    task_id: r.task_id,
                    container_ref: r.container_ref,
                    branch: r.branch,
                    agent_working_dir: r.agent_working_dir,
                    setup_completed_at: None,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                    archived: r.archived,
                    pinned: r.pinned,
                    name: r.name,
                })
                .collect()
        }
        None => {
            let records = sqlx::query!(
                r#"SELECT
                    id,
                    task_id,
                    container_ref,
                    branch,
                    agent_working_dir,
                    created_at,
                    updated_at,
                    archived,
                    pinned,
                    name
                FROM workspaces
                WHERE user_id = $1
                ORDER BY created_at DESC"#,
                user_id
            )
            .fetch_all(pool)
            .await?;

            records
                .into_iter()
                .map(|r| Workspace {
                    id: r.id,
                    task_id: r.task_id,
                    container_ref: r.container_ref,
                    branch: r.branch,
                    agent_working_dir: r.agent_working_dir,
                    setup_completed_at: None,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                    archived: r.archived,
                    pinned: r.pinned,
                    name: r.name,
                })
                .collect()
        }
    };

    Ok(workspaces)
}

/// Create a new workspace for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID who owns this workspace
/// * `data` - Workspace creation data
/// * `id` - Pre-generated UUID for the workspace
/// * `task_id` - Task ID this workspace belongs to
///
/// # Returns
///
/// The created workspace.
pub async fn create_for_user(
    pool: &PgPool,
    user_id: Uuid,
    data: &CreateWorkspace,
    id: Uuid,
    task_id: Uuid,
) -> Result<Workspace, sqlx::Error> {
    let record = sqlx::query!(
        r#"INSERT INTO workspaces (id, user_id, task_id, branch, agent_working_dir)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING
            id,
            task_id,
            container_ref,
            branch,
            agent_working_dir,
            created_at,
            updated_at,
            archived,
            pinned,
            name"#,
        id,
        user_id,
        task_id,
        data.branch,
        data.agent_working_dir,
    )
    .fetch_one(pool)
    .await?;

    Ok(Workspace {
        id: record.id,
        task_id: record.task_id,
        container_ref: record.container_ref,
        branch: record.branch,
        agent_working_dir: record.agent_working_dir,
        setup_completed_at: None,
        created_at: record.created_at,
        updated_at: record.updated_at,
        archived: record.archived,
        pinned: record.pinned,
        name: record.name,
    })
}

/// Update container reference for a workspace, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to update
/// * `container_ref` - New container reference
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn update_container_ref_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    container_ref: &str,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE workspaces SET container_ref = $3, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        workspace_id,
        user_id,
        container_ref
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Clear container reference for a workspace, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to update
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn clear_container_ref_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE workspaces SET container_ref = NULL, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        workspace_id,
        user_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Touch the workspace's updated_at timestamp to prevent cleanup.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to touch
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn touch_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE workspaces SET updated_at = NOW() WHERE id = $1 AND user_id = $2",
        workspace_id,
        user_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Update workspace fields, ensuring it belongs to the specified user.
/// Only non-None values will be updated.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to update
/// * `archived` - Optional new archived value
/// * `pinned` - Optional new pinned value
/// * `name` - Optional new name (pass Some("") to clear, Some("foo") to set, None to leave unchanged)
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn update_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    archived: Option<bool>,
    pinned: Option<bool>,
    name: Option<&str>,
) -> Result<(), sqlx::Error> {
    // Convert empty string to None for name field (to store as NULL)
    let name_value = name.filter(|s| !s.is_empty());
    let name_provided = name.is_some();

    let result = sqlx::query!(
        r#"UPDATE workspaces SET
            archived = COALESCE($3, archived),
            pinned = COALESCE($4, pinned),
            name = CASE WHEN $5 THEN $6 ELSE name END,
            updated_at = NOW()
        WHERE id = $1 AND user_id = $2"#,
        workspace_id,
        user_id,
        archived,
        pinned,
        name_provided,
        name_value,
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Set archived status for a workspace, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to update
/// * `archived` - New archived status
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn set_archived_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    archived: bool,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE workspaces SET archived = $3, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        workspace_id,
        user_id,
        archived
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Update branch name for a workspace, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to update
/// * `new_branch_name` - New branch name
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn update_branch_name_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
    new_branch_name: &str,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE workspaces SET branch = $3, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        workspace_id,
        user_id,
        new_branch_name
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Delete a workspace, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Workspace ID to delete
///
/// # Returns
///
/// The number of rows deleted (0 or 1).
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM workspaces WHERE id = $1 AND user_id = $2",
        id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Count total workspaces for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
///
/// # Returns
///
/// The count of workspaces owned by the user.
pub async fn count_all_for_user(pool: &PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    let result = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM workspaces WHERE user_id = $1"#,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(result)
}

/// Find all workspaces with status for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `archived` - Optional filter by archived status
/// * `limit` - Optional limit on results
///
/// # Returns
///
/// A vector of workspaces with status information.
pub async fn find_all_with_status_for_user(
    pool: &PgPool,
    user_id: Uuid,
    archived: Option<bool>,
    limit: Option<i64>,
) -> Result<Vec<WorkspaceWithStatus>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            w.id,
            w.task_id,
            w.container_ref,
            w.branch,
            w.agent_working_dir,
            w.created_at,
            w.updated_at,
            w.archived,
            w.pinned,
            w.name,

            CASE WHEN EXISTS (
                SELECT 1
                FROM sessions s
                JOIN execution_processes ep ON ep.session_id = s.id
                WHERE s.workspace_id = w.id
                  AND ep.status = 'running'
                  AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
                LIMIT 1
            ) THEN TRUE ELSE FALSE END AS "is_running!",

            CASE WHEN (
                SELECT ep.status
                FROM sessions s
                JOIN execution_processes ep ON ep.session_id = s.id
                WHERE s.workspace_id = w.id
                  AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
                ORDER BY ep.created_at DESC
                LIMIT 1
            ) IN ('failed','killed') THEN TRUE ELSE FALSE END AS "is_errored!"

        FROM workspaces w
        WHERE w.user_id = $1
        ORDER BY w.updated_at DESC"#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let mut workspaces: Vec<WorkspaceWithStatus> = records
        .into_iter()
        .map(|rec| WorkspaceWithStatus {
            workspace: Workspace {
                id: rec.id,
                task_id: rec.task_id,
                container_ref: rec.container_ref,
                branch: rec.branch,
                agent_working_dir: rec.agent_working_dir,
                setup_completed_at: None,
                created_at: rec.created_at,
                updated_at: rec.updated_at,
                archived: rec.archived,
                pinned: rec.pinned,
                name: rec.name,
            },
            is_running: rec.is_running,
            is_errored: rec.is_errored,
        })
        // Apply archived filter if provided
        .filter(|ws| archived.is_none_or(|a| ws.workspace.archived == a))
        .collect();

    // Apply limit if provided
    if let Some(lim) = limit {
        workspaces.truncate(lim as usize);
    }

    Ok(workspaces)
}

/// Find a workspace by ID with status, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Workspace ID to find
///
/// # Returns
///
/// The workspace with status if found and owned by the user, None otherwise.
pub async fn find_by_id_with_status_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<WorkspaceWithStatus>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            w.id,
            w.task_id,
            w.container_ref,
            w.branch,
            w.agent_working_dir,
            w.created_at,
            w.updated_at,
            w.archived,
            w.pinned,
            w.name,

            CASE WHEN EXISTS (
                SELECT 1
                FROM sessions s
                JOIN execution_processes ep ON ep.session_id = s.id
                WHERE s.workspace_id = w.id
                  AND ep.status = 'running'
                  AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
                LIMIT 1
            ) THEN TRUE ELSE FALSE END AS "is_running!",

            CASE WHEN (
                SELECT ep.status
                FROM sessions s
                JOIN execution_processes ep ON ep.session_id = s.id
                WHERE s.workspace_id = w.id
                  AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
                ORDER BY ep.created_at DESC
                LIMIT 1
            ) IN ('failed','killed') THEN TRUE ELSE FALSE END AS "is_errored!"

        FROM workspaces w
        WHERE w.id = $1 AND w.user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|rec| WorkspaceWithStatus {
        workspace: Workspace {
            id: rec.id,
            task_id: rec.task_id,
            container_ref: rec.container_ref,
            branch: rec.branch,
            agent_working_dir: rec.agent_working_dir,
            setup_completed_at: None,
            created_at: rec.created_at,
            updated_at: rec.updated_at,
            archived: rec.archived,
            pinned: rec.pinned,
            name: rec.name,
        },
        is_running: rec.is_running,
        is_errored: rec.is_errored,
    }))
}

#[cfg(test)]
mod tests {
    // Integration tests would go here, requiring a running PostgreSQL instance
    // and are marked with #[ignore] to not run in normal test suites
}
