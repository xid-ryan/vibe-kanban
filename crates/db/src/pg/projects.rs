//! PostgreSQL queries for projects with user_id filtering.
//!
//! This module provides PostgreSQL-specific query functions for the projects table
//! that include user_id filtering for multi-tenant isolation in Kubernetes deployments.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::project::{CreateProject, Project, UpdateProject};

/// Count projects for a specific user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
///
/// # Returns
///
/// The count of projects owned by the user.
pub async fn count_for_user(pool: &PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    let result = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM projects WHERE user_id = $1"#,
        user_id
    )
    .fetch_one(pool)
    .await?;

    Ok(result)
}

/// Find all projects for a specific user, ordered by creation date descending.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
///
/// # Returns
///
/// A vector of projects owned by the user.
pub async fn find_all_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Project>, sqlx::Error> {
    // Note: PostgreSQL schema doesn't have default_agent_working_dir column
    // We return None for that field to maintain compatibility with the Project struct
    let records = sqlx::query!(
        r#"SELECT
            id,
            name,
            remote_project_id,
            created_at,
            updated_at
        FROM projects
        WHERE user_id = $1
        ORDER BY created_at DESC"#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| Project {
            id: r.id,
            name: r.name,
            default_agent_working_dir: None,
            remote_project_id: r.remote_project_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Find a project by ID, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Project ID to find
///
/// # Returns
///
/// The project if found and owned by the user, None otherwise.
pub async fn find_by_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Project>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            id,
            name,
            remote_project_id,
            created_at,
            updated_at
        FROM projects
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| Project {
        id: r.id,
        name: r.name,
        default_agent_working_dir: None,
        remote_project_id: r.remote_project_id,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Find a project by remote_project_id, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `remote_project_id` - Remote project ID to find
///
/// # Returns
///
/// The project if found and owned by the user, None otherwise.
pub async fn find_by_remote_project_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    remote_project_id: Uuid,
) -> Result<Option<Project>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            id,
            name,
            remote_project_id,
            created_at,
            updated_at
        FROM projects
        WHERE remote_project_id = $1 AND user_id = $2
        LIMIT 1"#,
        remote_project_id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| Project {
        id: r.id,
        name: r.name,
        default_agent_working_dir: None,
        remote_project_id: r.remote_project_id,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Create a new project for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID who owns this project
/// * `data` - Project creation data
/// * `project_id` - Pre-generated UUID for the project
///
/// # Returns
///
/// The created project.
pub async fn create_for_user(
    pool: &PgPool,
    user_id: Uuid,
    data: &CreateProject,
    project_id: Uuid,
) -> Result<Project, sqlx::Error> {
    let record = sqlx::query!(
        r#"INSERT INTO projects (id, user_id, name)
        VALUES ($1, $2, $3)
        RETURNING
            id,
            name,
            remote_project_id,
            created_at,
            updated_at"#,
        project_id,
        user_id,
        data.name,
    )
    .fetch_one(pool)
    .await?;

    Ok(Project {
        id: record.id,
        name: record.name,
        default_agent_working_dir: None,
        remote_project_id: record.remote_project_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Update an existing project, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Project ID to update
/// * `payload` - Update data
///
/// # Returns
///
/// The updated project.
pub async fn update_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    payload: &UpdateProject,
) -> Result<Project, sqlx::Error> {
    let existing = find_by_id_for_user(pool, user_id, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    let name = payload.name.clone().unwrap_or(existing.name);

    let record = sqlx::query!(
        r#"UPDATE projects
        SET name = $3, updated_at = NOW()
        WHERE id = $1 AND user_id = $2
        RETURNING
            id,
            name,
            remote_project_id,
            created_at,
            updated_at"#,
        id,
        user_id,
        name,
    )
    .fetch_one(pool)
    .await?;

    Ok(Project {
        id: record.id,
        name: record.name,
        default_agent_working_dir: None,
        remote_project_id: record.remote_project_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Set the remote_project_id for a project, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Project ID to update
/// * `remote_project_id` - Remote project ID to set (or None to clear)
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn set_remote_project_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    remote_project_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        r#"UPDATE projects
        SET remote_project_id = $3, updated_at = NOW()
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id,
        remote_project_id
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Delete a project, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Project ID to delete
///
/// # Returns
///
/// The number of rows deleted (0 or 1).
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM projects WHERE id = $1 AND user_id = $2",
        id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Find the most actively used projects based on recent workspace activity.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `limit` - Maximum number of projects to return
///
/// # Returns
///
/// A vector of the most active projects for the user.
pub async fn find_most_active_for_user(
    pool: &PgPool,
    user_id: Uuid,
    limit: i32,
) -> Result<Vec<Project>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT p.id, p.name, p.remote_project_id, p.created_at, p.updated_at
        FROM projects p
        WHERE p.user_id = $1
          AND p.id IN (
              SELECT DISTINCT t.project_id
              FROM tasks t
              INNER JOIN workspaces w ON w.task_id = t.id
              WHERE t.user_id = $1
              ORDER BY w.updated_at DESC
          )
        LIMIT $2"#,
        user_id,
        limit as i64
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| Project {
            id: r.id,
            name: r.name,
            default_agent_working_dir: None,
            remote_project_id: r.remote_project_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    // Integration tests would go here, requiring a running PostgreSQL instance
    // and are marked with #[ignore] to not run in normal test suites
}
