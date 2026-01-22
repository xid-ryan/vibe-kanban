//! PostgreSQL queries for tasks with user_id filtering.
//!
//! This module provides PostgreSQL-specific query functions for the tasks table
//! that include user_id filtering for multi-tenant isolation in Kubernetes deployments.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::task::{CreateTask, Task, TaskStatus, TaskWithAttemptStatus};

/// Find all tasks for a project, ensuring they belong to the specified user.
/// Returns tasks with attempt status information.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `project_id` - Project ID to find tasks for
///
/// # Returns
///
/// A vector of tasks with attempt status information.
pub async fn find_by_project_id_with_attempt_status_for_user(
    pool: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<TaskWithAttemptStatus>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            t.id,
            t.project_id,
            t.title,
            t.description,
            t.status,
            t.parent_workspace_id,
            t.created_at,
            t.updated_at,

            CASE WHEN EXISTS (
                SELECT 1
                FROM workspaces w
                JOIN sessions s ON s.workspace_id = w.id
                JOIN execution_processes ep ON ep.session_id = s.id
                WHERE w.task_id = t.id
                  AND ep.status = 'running'
                  AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
                LIMIT 1
            ) THEN TRUE ELSE FALSE END AS "has_in_progress_attempt!",

            CASE WHEN (
                SELECT ep.status
                FROM workspaces w
                JOIN sessions s ON s.workspace_id = w.id
                JOIN execution_processes ep ON ep.session_id = s.id
                WHERE w.task_id = t.id
                  AND ep.run_reason IN ('setupscript','cleanupscript','codingagent')
                ORDER BY ep.created_at DESC
                LIMIT 1
            ) IN ('failed','killed') THEN TRUE ELSE FALSE END AS "last_attempt_failed!",

            COALESCE(
                (SELECT s.executor
                 FROM workspaces w
                 JOIN sessions s ON s.workspace_id = w.id
                 WHERE w.task_id = t.id
                 ORDER BY s.created_at DESC
                 LIMIT 1),
                ''
            ) AS "executor!"

        FROM tasks t
        WHERE t.project_id = $1 AND t.user_id = $2
        ORDER BY t.created_at DESC"#,
        project_id,
        user_id
    )
    .fetch_all(pool)
    .await?;

    let tasks = records
        .into_iter()
        .map(|rec| {
            let status = match rec.status.as_str() {
                "todo" => TaskStatus::Todo,
                "inprogress" => TaskStatus::InProgress,
                "inreview" => TaskStatus::InReview,
                "done" => TaskStatus::Done,
                "cancelled" => TaskStatus::Cancelled,
                _ => TaskStatus::Todo,
            };

            TaskWithAttemptStatus {
                task: Task {
                    id: rec.id,
                    project_id: rec.project_id,
                    title: rec.title,
                    description: rec.description,
                    status,
                    parent_workspace_id: rec.parent_workspace_id,
                    created_at: rec.created_at,
                    updated_at: rec.updated_at,
                },
                has_in_progress_attempt: rec.has_in_progress_attempt,
                last_attempt_failed: rec.last_attempt_failed,
                executor: rec.executor,
            }
        })
        .collect();

    Ok(tasks)
}

/// Find a task by ID, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Task ID to find
///
/// # Returns
///
/// The task if found and owned by the user, None otherwise.
pub async fn find_by_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Task>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            id,
            project_id,
            title,
            description,
            status,
            parent_workspace_id,
            created_at,
            updated_at
        FROM tasks
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|rec| {
        let status = match rec.status.as_str() {
            "todo" => TaskStatus::Todo,
            "inprogress" => TaskStatus::InProgress,
            "inreview" => TaskStatus::InReview,
            "done" => TaskStatus::Done,
            "cancelled" => TaskStatus::Cancelled,
            _ => TaskStatus::Todo,
        };

        Task {
            id: rec.id,
            project_id: rec.project_id,
            title: rec.title,
            description: rec.description,
            status,
            parent_workspace_id: rec.parent_workspace_id,
            created_at: rec.created_at,
            updated_at: rec.updated_at,
        }
    }))
}

/// Create a new task for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID who owns this task
/// * `data` - Task creation data
/// * `task_id` - Pre-generated UUID for the task
///
/// # Returns
///
/// The created task.
pub async fn create_for_user(
    pool: &PgPool,
    user_id: Uuid,
    data: &CreateTask,
    task_id: Uuid,
) -> Result<Task, sqlx::Error> {
    let status = data.status.clone().unwrap_or_default();
    let status_str = status.to_string().to_lowercase();

    let record = sqlx::query!(
        r#"INSERT INTO tasks (id, user_id, project_id, title, description, status, parent_workspace_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING
            id,
            project_id,
            title,
            description,
            status,
            parent_workspace_id,
            created_at,
            updated_at"#,
        task_id,
        user_id,
        data.project_id,
        data.title,
        data.description,
        status_str,
        data.parent_workspace_id
    )
    .fetch_one(pool)
    .await?;

    let result_status = match record.status.as_str() {
        "todo" => TaskStatus::Todo,
        "inprogress" => TaskStatus::InProgress,
        "inreview" => TaskStatus::InReview,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Todo,
    };

    Ok(Task {
        id: record.id,
        project_id: record.project_id,
        title: record.title,
        description: record.description,
        status: result_status,
        parent_workspace_id: record.parent_workspace_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Update an existing task, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Task ID to update
/// * `project_id` - Project ID for validation
/// * `title` - New title
/// * `description` - New description
/// * `status` - New status
/// * `parent_workspace_id` - New parent workspace ID
///
/// # Returns
///
/// The updated task.
pub async fn update_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: Option<String>,
    status: TaskStatus,
    parent_workspace_id: Option<Uuid>,
) -> Result<Task, sqlx::Error> {
    let status_str = status.to_string().to_lowercase();

    let record = sqlx::query!(
        r#"UPDATE tasks
        SET title = $4, description = $5, status = $6, parent_workspace_id = $7, updated_at = NOW()
        WHERE id = $1 AND user_id = $2 AND project_id = $3
        RETURNING
            id,
            project_id,
            title,
            description,
            status,
            parent_workspace_id,
            created_at,
            updated_at"#,
        id,
        user_id,
        project_id,
        title,
        description,
        status_str,
        parent_workspace_id
    )
    .fetch_one(pool)
    .await?;

    let result_status = match record.status.as_str() {
        "todo" => TaskStatus::Todo,
        "inprogress" => TaskStatus::InProgress,
        "inreview" => TaskStatus::InReview,
        "done" => TaskStatus::Done,
        "cancelled" => TaskStatus::Cancelled,
        _ => TaskStatus::Todo,
    };

    Ok(Task {
        id: record.id,
        project_id: record.project_id,
        title: record.title,
        description: record.description,
        status: result_status,
        parent_workspace_id: record.parent_workspace_id,
        created_at: record.created_at,
        updated_at: record.updated_at,
    })
}

/// Update only the status of a task, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Task ID to update
/// * `status` - New status
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn update_status_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    status: TaskStatus,
) -> Result<(), sqlx::Error> {
    let status_str = status.to_string().to_lowercase();

    let result = sqlx::query!(
        "UPDATE tasks SET status = $3, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        id,
        user_id,
        status_str
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Delete a task, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Task ID to delete
///
/// # Returns
///
/// The number of rows deleted (0 or 1).
pub async fn delete_for_user(pool: &PgPool, user_id: Uuid, id: Uuid) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM tasks WHERE id = $1 AND user_id = $2",
        id,
        user_id
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Find children tasks by workspace ID, ensuring they belong to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Parent workspace ID
///
/// # Returns
///
/// A vector of child tasks.
pub async fn find_children_by_workspace_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<Task>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            id,
            project_id,
            title,
            description,
            status,
            parent_workspace_id,
            created_at,
            updated_at
        FROM tasks
        WHERE parent_workspace_id = $1 AND user_id = $2
        ORDER BY created_at DESC"#,
        workspace_id,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|rec| {
            let status = match rec.status.as_str() {
                "todo" => TaskStatus::Todo,
                "inprogress" => TaskStatus::InProgress,
                "inreview" => TaskStatus::InReview,
                "done" => TaskStatus::Done,
                "cancelled" => TaskStatus::Cancelled,
                _ => TaskStatus::Todo,
            };

            Task {
                id: rec.id,
                project_id: rec.project_id,
                title: rec.title,
                description: rec.description,
                status,
                parent_workspace_id: rec.parent_workspace_id,
                created_at: rec.created_at,
                updated_at: rec.updated_at,
            }
        })
        .collect())
}

/// Nullify parent_workspace_id for all tasks that reference the given workspace ID.
/// This breaks parent-child relationships before deleting a parent task.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to clear references to
///
/// # Returns
///
/// The number of rows affected.
pub async fn nullify_children_by_workspace_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE tasks SET parent_workspace_id = NULL, updated_at = NOW() WHERE parent_workspace_id = $1 AND user_id = $2",
        workspace_id,
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
