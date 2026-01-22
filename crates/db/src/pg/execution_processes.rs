//! PostgreSQL queries for execution_processes with user_id filtering.
//!
//! This module provides PostgreSQL-specific query functions for the execution_processes table
//! that include user_id filtering for multi-tenant isolation in Kubernetes deployments.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::execution_process::{
    ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus,
    ExecutorActionField, LatestProcessInfo,
};

/// Find execution process by ID, ensuring it belongs to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Execution process ID to find
///
/// # Returns
///
/// The execution process if found and owned by the user, None otherwise.
pub async fn find_by_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<ExecutionProcess>, sqlx::Error> {
    let record = sqlx::query!(
        r#"SELECT
            id,
            session_id,
            run_reason,
            executor_action,
            status,
            exit_code,
            dropped,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM execution_processes
        WHERE id = $1 AND user_id = $2"#,
        id,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|r| ExecutionProcess {
        id: r.id,
        session_id: r.session_id,
        run_reason: match r.run_reason.as_str() {
            "setupscript" => ExecutionProcessRunReason::SetupScript,
            "cleanupscript" => ExecutionProcessRunReason::CleanupScript,
            "codingagent" => ExecutionProcessRunReason::CodingAgent,
            "devserver" => ExecutionProcessRunReason::DevServer,
            _ => ExecutionProcessRunReason::CodingAgent,
        },
        executor_action: serde_json::from_value(r.executor_action)
            .unwrap_or(sqlx::types::Json(ExecutorActionField::Other(serde_json::Value::Null))),
        status: match r.status.as_str() {
            "running" => ExecutionProcessStatus::Running,
            "completed" => ExecutionProcessStatus::Completed,
            "failed" => ExecutionProcessStatus::Failed,
            "killed" => ExecutionProcessStatus::Killed,
            _ => ExecutionProcessStatus::Running,
        },
        exit_code: r.exit_code,
        dropped: r.dropped,
        started_at: r.started_at,
        completed_at: r.completed_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }))
}

/// Find all execution processes for a session, ensuring they belong to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `session_id` - Session ID to find processes for
/// * `show_soft_deleted` - Whether to include dropped processes
///
/// # Returns
///
/// A vector of execution processes.
pub async fn find_by_session_id_for_user(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    show_soft_deleted: bool,
) -> Result<Vec<ExecutionProcess>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            id,
            session_id,
            run_reason,
            executor_action,
            status,
            exit_code,
            dropped,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM execution_processes
        WHERE session_id = $1 AND user_id = $2
          AND ($3 OR dropped = FALSE)
        ORDER BY created_at ASC"#,
        session_id,
        user_id,
        show_soft_deleted
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| ExecutionProcess {
            id: r.id,
            session_id: r.session_id,
            run_reason: match r.run_reason.as_str() {
                "setupscript" => ExecutionProcessRunReason::SetupScript,
                "cleanupscript" => ExecutionProcessRunReason::CleanupScript,
                "codingagent" => ExecutionProcessRunReason::CodingAgent,
                "devserver" => ExecutionProcessRunReason::DevServer,
                _ => ExecutionProcessRunReason::CodingAgent,
            },
            executor_action: serde_json::from_value(r.executor_action)
                .unwrap_or(sqlx::types::Json(ExecutorActionField::Other(serde_json::Value::Null))),
            status: match r.status.as_str() {
                "running" => ExecutionProcessStatus::Running,
                "completed" => ExecutionProcessStatus::Completed,
                "failed" => ExecutionProcessStatus::Failed,
                "killed" => ExecutionProcessStatus::Killed,
                _ => ExecutionProcessStatus::Running,
            },
            exit_code: r.exit_code,
            dropped: r.dropped,
            started_at: r.started_at,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Find running execution processes for a user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
///
/// # Returns
///
/// A vector of running execution processes.
pub async fn find_running_for_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<ExecutionProcess>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            id,
            session_id,
            run_reason,
            executor_action,
            status,
            exit_code,
            dropped,
            started_at,
            completed_at,
            created_at,
            updated_at
        FROM execution_processes
        WHERE user_id = $1 AND status = 'running'
        ORDER BY created_at ASC"#,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| ExecutionProcess {
            id: r.id,
            session_id: r.session_id,
            run_reason: match r.run_reason.as_str() {
                "setupscript" => ExecutionProcessRunReason::SetupScript,
                "cleanupscript" => ExecutionProcessRunReason::CleanupScript,
                "codingagent" => ExecutionProcessRunReason::CodingAgent,
                "devserver" => ExecutionProcessRunReason::DevServer,
                _ => ExecutionProcessRunReason::CodingAgent,
            },
            executor_action: serde_json::from_value(r.executor_action)
                .unwrap_or(sqlx::types::Json(ExecutorActionField::Other(serde_json::Value::Null))),
            status: ExecutionProcessStatus::Running,
            exit_code: r.exit_code,
            dropped: r.dropped,
            started_at: r.started_at,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Find running dev servers for a specific project, ensuring they belong to the specified user.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `project_id` - Project ID to filter by
///
/// # Returns
///
/// A vector of running dev server execution processes.
pub async fn find_running_dev_servers_by_project_for_user(
    pool: &PgPool,
    user_id: Uuid,
    project_id: Uuid,
) -> Result<Vec<ExecutionProcess>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT ep.id, ep.session_id, ep.run_reason, ep.executor_action,
                  ep.status, ep.exit_code, ep.dropped, ep.started_at,
                  ep.completed_at, ep.created_at, ep.updated_at
        FROM execution_processes ep
        JOIN sessions s ON ep.session_id = s.id
        JOIN workspaces w ON s.workspace_id = w.id
        JOIN tasks t ON w.task_id = t.id
        WHERE ep.user_id = $1
          AND ep.status = 'running'
          AND ep.run_reason = 'devserver'
          AND t.project_id = $2
        ORDER BY ep.created_at ASC"#,
        user_id,
        project_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| ExecutionProcess {
            id: r.id,
            session_id: r.session_id,
            run_reason: ExecutionProcessRunReason::DevServer,
            executor_action: serde_json::from_value(r.executor_action)
                .unwrap_or(sqlx::types::Json(ExecutorActionField::Other(serde_json::Value::Null))),
            status: ExecutionProcessStatus::Running,
            exit_code: r.exit_code,
            dropped: r.dropped,
            started_at: r.started_at,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Check if there are running non-dev-server processes for a workspace, ensuring user ownership.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to check
///
/// # Returns
///
/// True if there are running non-dev-server processes.
pub async fn has_running_non_dev_server_processes_for_workspace_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let count: i64 = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!"
        FROM execution_processes ep
        JOIN sessions s ON ep.session_id = s.id
        WHERE s.workspace_id = $1
          AND ep.user_id = $2
          AND ep.status = 'running'
          AND ep.run_reason != 'devserver'"#,
        workspace_id,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

/// Find running dev servers for a specific workspace, ensuring user ownership.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `workspace_id` - Workspace ID to filter by
///
/// # Returns
///
/// A vector of running dev server execution processes.
pub async fn find_running_dev_servers_by_workspace_for_user(
    pool: &PgPool,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<ExecutionProcess>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT ep.id, ep.session_id, ep.run_reason, ep.executor_action,
                  ep.status, ep.exit_code, ep.dropped, ep.started_at,
                  ep.completed_at, ep.created_at, ep.updated_at
        FROM execution_processes ep
        JOIN sessions s ON ep.session_id = s.id
        WHERE s.workspace_id = $1
          AND ep.user_id = $2
          AND ep.status = 'running'
          AND ep.run_reason = 'devserver'
        ORDER BY ep.created_at DESC"#,
        workspace_id,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| ExecutionProcess {
            id: r.id,
            session_id: r.session_id,
            run_reason: ExecutionProcessRunReason::DevServer,
            executor_action: serde_json::from_value(r.executor_action)
                .unwrap_or(sqlx::types::Json(ExecutorActionField::Other(serde_json::Value::Null))),
            status: ExecutionProcessStatus::Running,
            exit_code: r.exit_code,
            dropped: r.dropped,
            started_at: r.started_at,
            completed_at: r.completed_at,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
        .collect())
}

/// Update execution process status and completion info, ensuring user ownership.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `id` - Execution process ID to update
/// * `status` - New status
/// * `exit_code` - Optional exit code
///
/// # Returns
///
/// Ok(()) if successful.
pub async fn update_completion_for_user(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
    status: ExecutionProcessStatus,
    exit_code: Option<i64>,
) -> Result<(), sqlx::Error> {
    let completed_at = if matches!(status, ExecutionProcessStatus::Running) {
        None
    } else {
        Some(chrono::Utc::now())
    };

    let status_str = match status {
        ExecutionProcessStatus::Running => "running",
        ExecutionProcessStatus::Completed => "completed",
        ExecutionProcessStatus::Failed => "failed",
        ExecutionProcessStatus::Killed => "killed",
    };

    let result = sqlx::query!(
        "UPDATE execution_processes SET status = $3, exit_code = $4, completed_at = $5, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        id,
        user_id,
        status_str,
        exit_code,
        completed_at
    )
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Soft-drop processes at and after the specified boundary, ensuring user ownership.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `session_id` - Session ID
/// * `boundary_process_id` - Process ID marking the boundary
///
/// # Returns
///
/// The number of rows affected.
pub async fn drop_at_and_after_for_user(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    boundary_process_id: Uuid,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query!(
        r#"UPDATE execution_processes
        SET dropped = TRUE, updated_at = NOW()
        WHERE session_id = $1
          AND user_id = $2
          AND created_at >= (SELECT created_at FROM execution_processes WHERE id = $3)
          AND dropped = FALSE"#,
        session_id,
        user_id,
        boundary_process_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() as i64)
}

/// Fetch latest execution process info for all workspaces with the given archived status.
///
/// # Arguments
///
/// * `pool` - PostgreSQL connection pool
/// * `user_id` - User ID for filtering
/// * `archived` - Whether to filter by archived status
///
/// # Returns
///
/// A vector of latest process info for workspaces.
pub async fn find_latest_for_workspaces_for_user(
    pool: &PgPool,
    user_id: Uuid,
    archived: bool,
) -> Result<Vec<LatestProcessInfo>, sqlx::Error> {
    let records = sqlx::query!(
        r#"SELECT
            s.workspace_id,
            ep.id as execution_process_id,
            ep.session_id,
            ep.status,
            ep.completed_at
        FROM execution_processes ep
        JOIN sessions s ON ep.session_id = s.id
        JOIN workspaces w ON s.workspace_id = w.id
        WHERE w.archived = $1
          AND ep.user_id = $2
          AND ep.run_reason IN ('codingagent', 'setupscript', 'cleanupscript')
          AND ep.dropped = FALSE
          AND ep.created_at = (
              SELECT MAX(ep2.created_at)
              FROM execution_processes ep2
              JOIN sessions s2 ON ep2.session_id = s2.id
              WHERE s2.workspace_id = s.workspace_id
                AND ep2.user_id = $2
                AND ep2.run_reason IN ('codingagent', 'setupscript', 'cleanupscript')
                AND ep2.dropped = FALSE
          )"#,
        archived,
        user_id
    )
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|r| LatestProcessInfo {
            workspace_id: r.workspace_id,
            execution_process_id: r.execution_process_id,
            session_id: r.session_id,
            status: match r.status.as_str() {
                "running" => ExecutionProcessStatus::Running,
                "completed" => ExecutionProcessStatus::Completed,
                "failed" => ExecutionProcessStatus::Failed,
                "killed" => ExecutionProcessStatus::Killed,
                _ => ExecutionProcessStatus::Running,
            },
            completed_at: r.completed_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    // Integration tests would go here, requiring a running PostgreSQL instance
    // and are marked with #[ignore] to not run in normal test suites
}
