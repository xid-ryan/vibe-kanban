//! PostgreSQL queries for repos with user_id filtering.
//!
//! This module provides PostgreSQL-specific query functions for the repos table
//! that include user_id filtering for multi-tenant isolation in Kubernetes deployments.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::repo::{Repo, UpdateRepo};

/// Find a repo by ID, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Repo ID to find
///
/// # Returns
///
/// The repo if found and owned by the user, None otherwise.
pub async fn find_by_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Repo>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            id,
            path,
            name,
            display_name,
            setup_script,
            cleanup_script,
            copy_files,
            parallel_setup_script,
            dev_server_script,
            created_at,
            updated_at
        FROM repos
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| Repo {
        id: r.id,
        path: PathBuf::from(&r.path),
        name: r.name,
        display_name: r.display_name,
        setup_script: r.setup_script,
        cleanup_script: r.cleanup_script,
        copy_files: r.copy_files,
        parallel_setup_script: r.parallel_setup_script,
        dev_server_script: r.dev_server_script,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Find repos by multiple IDs, ensuring they belong to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `ids` - Repo IDs to find
///
/// # Returns
///
/// A vector of repos owned by the user.
pub async fn find_by_ids_for_user(
    pool: &PgPool,
    user_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Repo>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // PostgreSQL supports array parameters, but for consistency we'll fetch individually
    let mut repos = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(repo) = find_by_id_for_user(pool, user_id, *id).await? {
            repos.push(repo);
        }
    }
    Ok(repos)
}

/// List all repos for a user, ordered by display name.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
///
/// # Returns
///
/// A vector of repos owned by the user.
pub async fn list_all_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Repo>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            id,
            path,
            name,
            display_name,
            setup_script,
            cleanup_script,
            copy_files,
            parallel_setup_script,
            dev_server_script,
            created_at,
            updated_at
        FROM repos
        WHERE user_id = $1
        ORDER BY display_name ASC"#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| Repo {
            id: r.id,
            path: PathBuf::from(&r.path),
            name: r.name,
            display_name: r.display_name,
            setup_script: r.setup_script,
            cleanup_script: r.cleanup_script,
            copy_files: r.copy_files,
            parallel_setup_script: r.parallel_setup_script,
            dev_server_script: r.dev_server_script,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Find or create a repo for a user.
///
/// Uses INSERT ... ON CONFLICT to handle race conditions atomically.
/// For PostgreSQL, the unique constraint is on (user_id, path).
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID who owns this repo
/// * `path` - Path to the repository
/// * `display_name` - Display name for the repo
///
/// # Returns
///
/// The found or newly created repo.
pub async fn find_or_create_for_user(
    pool: &PgPool,
    user_id: Uuid,
    path: &Path,
    display_name: &str,
) -> Result<Repo, sqlx::Error> {
    let path_str = path.to_string_lossy().to_string();
    let id = Uuid::new_v4();
    let repo_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| id.to_string());

    let record = sqlx::query!(
        r#"INSERT INTO repos (id, user_id, path, name, display_name)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (user_id, path) DO UPDATE SET updated_at = NOW()
        RETURNING
            id,
            path,
            name,
            display_name,
            setup_script,
            cleanup_script,
            copy_files,
            parallel_setup_script,
            dev_server_script,
            created_at,
            updated_at"#,
        id,
        user_id,
        path_str,
        repo_name,
        display_name,
    )
    .fetch_one(pool)
    .await?;

    Ok(Repo {
        id: record.id,
        path: PathBuf::from(&record.path),
        name: record.name,
        display_name: record.display_name,
        setup_script: record.setup_script,
        cleanup_script: record.cleanup_script,
        copy_files: record.copy_files,
        parallel_setup_script: record.parallel_setup_script,
        dev_server_script: record.dev_server_script,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Update a repo, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Repo ID to update
/// * `payload` - Update data
///
/// # Returns
///
/// The updated repo.
pub async fn update_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    payload: &UpdateRepo,
) -> Result<Repo, sqlx::Error> {
    let existing = find_by_id_for_user(pool, user_id, id)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

    // None = don't update (use existing)
    // Some(None) = set to NULL
    // Some(Some(v)) = set to v
    let display_name = match &payload.display_name {
        None => existing.display_name,
        Some(v) => v.clone().unwrap_or_default(),
    };
    let setup_script = match &payload.setup_script {
        None => existing.setup_script,
        Some(v) => v.clone(),
    };
    let cleanup_script = match &payload.cleanup_script {
        None => existing.cleanup_script,
        Some(v) => v.clone(),
    };
    let copy_files = match &payload.copy_files {
        None => existing.copy_files,
        Some(v) => v.clone(),
    };
    let parallel_setup_script = match &payload.parallel_setup_script {
        None => existing.parallel_setup_script,
        Some(v) => v.unwrap_or(false),
    };
    let dev_server_script = match &payload.dev_server_script {
        None => existing.dev_server_script,
        Some(v) => v.clone(),
    };

    let record = sqlx::query!(
        r#"UPDATE repos
        SET display_name = $4,
            setup_script = $5,
            cleanup_script = $6,
            copy_files = $7,
            parallel_setup_script = $8,
            dev_server_script = $9,
            updated_at = NOW()
        WHERE id = $1 AND user_id = $2
        RETURNING
            id,
            path,
            name,
            display_name,
            setup_script,
            cleanup_script,
            copy_files,
            parallel_setup_script,
            dev_server_script,
            created_at,
            updated_at"#,
        id,
        user_id,
        existing.path.to_string_lossy().to_string(),
        display_name,
        setup_script,
        cleanup_script,
        copy_files,
        parallel_setup_script,
        dev_server_script,
    )
    .fetch_one(pool)
    .await?;

    Ok(Repo {
        id: record.id,
        path: PathBuf::from(&record.path),
        name: record.name,
        display_name: record.display_name,
        setup_script: record.setup_script,
        cleanup_script: record.cleanup_script,
        copy_files: record.copy_files,
        parallel_setup_script: record.parallel_setup_script,
        dev_server_script: record.dev_server_script,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Update repo name, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Repo ID to update
/// * `name` - New name
/// * `display_name` - New display name
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn update_name_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    name: &str,
    display_name: &str,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE repos SET name = $3, display_name = $4, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        id,
        user_id,
        name,
        display_name
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Delete a repo, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Repo ID to delete
///
/// # Returns
///
/// The number of rows deleted (0 or 1).
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM repos WHERE id = $1 AND user_id = $2",
        id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Delete orphaned repos (not referenced by any project_repos or workspace_repos)
/// for a specific user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
///
/// # Returns
///
/// The number of rows deleted.
pub async fn delete_orphaned_for_user(pool: &PgPool, user_id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"DELETE FROM repos
        WHERE user_id = $1
          AND id NOT IN (SELECT repo_id FROM project_repos)
          AND id NOT IN (SELECT repo_id FROM workspace_repos)"#,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Find repos for a project, ensuring they belong to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `project_id` - Project ID to find repos for
///
/// # Returns
///
/// A vector of repos linked to the project and owned by the user.
pub async fn find_repos_for_project_for_user(
    pool: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<Repo>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT r.id, r.path, r.name, r.display_name, r.setup_script,
                  r.cleanup_script, r.copy_files, r.parallel_setup_script,
                  r.dev_server_script, r.created_at, r.updated_at
        FROM repos r
        JOIN project_repos pr ON r.id = pr.repo_id
        WHERE pr.project_id = $1 AND r.user_id = $2
        ORDER BY r.display_name ASC"#,
        project_id,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| Repo {
            id: r.id,
            path: PathBuf::from(&r.path),
            name: r.name,
            display_name: r.display_name,
            setup_script: r.setup_script,
            cleanup_script: r.cleanup_script,
            copy_files: r.copy_files,
            parallel_setup_script: r.parallel_setup_script,
            dev_server_script: r.dev_server_script,
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
