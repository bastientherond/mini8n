//! Historique des exécutions côté serveur.

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::Json as SqlJson;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::workspaces::require_member;

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    id: Uuid,
    workflow_id: Option<Uuid>,
    workflow_name: String,
    trigger_kind: String,
    trigger_node: Option<String>,
    parent_id: Option<Uuid>,
    status: String,
    error: Option<String>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    duration_ms: Option<f64>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    workflow: Option<Uuid>,
    status: Option<String>,
    limit: Option<i64>,
    before: Option<DateTime<Utc>>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Vec<ExecutionSummary>>> {
    require_member(&state.db, ws, user.id).await?;
    let status = q
        .status
        .filter(|s| matches!(s.as_str(), "running" | "success" | "error"));
    Ok(Json(
        sqlx::query_as(
            "SELECT id, workflow_id, workflow_name, trigger_kind, trigger_node, parent_id, status, error,
                    started_at, finished_at, duration_ms
             FROM executions
             WHERE workspace_id = $1
               AND ($2::uuid IS NULL OR workflow_id = $2)
               AND ($3::text IS NULL OR status = $3)
               AND ($4::timestamptz IS NULL OR started_at < $4)
             ORDER BY started_at DESC
             LIMIT $5",
        )
        .bind(ws)
        .bind(q.workflow)
        .bind(status)
        .bind(q.before)
        .bind(q.limit.unwrap_or(50).clamp(1, 200))
        .fetch_all(&state.db)
        .await?,
    ))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionDetail {
    #[serde(flatten)]
    summary: ExecutionSummary,
    report: Option<Value>,
    output: Option<Value>,
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ExecutionDetail>> {
    require_member(&state.db, ws, user.id).await?;
    let summary: ExecutionSummary = sqlx::query_as(
        "SELECT id, workflow_id, workflow_name, trigger_kind, trigger_node, parent_id, status, error,
                started_at, finished_at, duration_ms
         FROM executions WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(ws)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let (report, output): (Option<SqlJson<Value>>, Option<SqlJson<Value>>) =
        sqlx::query_as("SELECT report, output FROM executions WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(ExecutionDetail {
        summary,
        report: report.map(|SqlJson(v)| v),
        output: output.map(|SqlJson(v)| v),
    }))
}
