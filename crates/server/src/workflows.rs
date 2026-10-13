//! Stockage des workflows d'un workspace (avec contrôle de version optimiste).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::validate;
use crate::workspaces::require_member;

const MAX_DEFINITION_BYTES: usize = 1024 * 1024;
const MAX_NODES: usize = 500;
const MAX_EDGES: usize = 2000;
const MAX_NOTES: usize = 200;
const MAX_NOTE_TEXT: usize = 10_000;
pub(crate) const MAX_WORKFLOWS: i64 = 500;

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    id: Uuid,
    name: String,
    version: i32,
    active: bool,
    updated_at: DateTime<Utc>,
    updated_by: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDoc {
    id: Uuid,
    name: String,
    version: i32,
    active: bool,
    /// Aperçu des données des exécutions conservé dans l'historique.
    keep_data: bool,
    definition: sqlx::types::Json<Value>,
    updated_at: DateTime<Utc>,
    /// Motif de désactivation automatique (déclencheur devenu invalide à la sauvegarde).
    #[sqlx(skip)]
    #[serde(skip_serializing_if = "Option::is_none")]
    deactivated: Option<String>,
}

/// Identifiant de nœud ou de lien : lettres, chiffres, `-` et `_`.
pub(crate) fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Contrôle la forme et la taille d'une définition de workflow.
pub(crate) fn check_definition(def: &Value) -> AppResult<()> {
    let bad = |m: &str| Err(AppError::bad(format!("Définition invalide : {m}")));
    if serde_json::to_vec(def).map_err(AppError::internal)?.len() > MAX_DEFINITION_BYTES {
        return bad("taille maximale de 1 Mo dépassée");
    }
    let Some(obj) = def.as_object() else {
        return bad("objet attendu");
    };
    let (Some(nodes), Some(edges)) = (
        obj.get("nodes").and_then(Value::as_array),
        obj.get("edges").and_then(Value::as_array),
    ) else {
        return bad("champs nodes et edges requis");
    };
    if nodes.len() > MAX_NODES || edges.len() > MAX_EDGES {
        return bad(&format!(
            "{MAX_NODES} nœuds et {MAX_EDGES} liens au maximum"
        ));
    }
    for n in nodes {
        let kind = n.pointer("/data/kind").and_then(Value::as_str);
        let pos_ok = ["/position/x", "/position/y"].iter().all(|p| {
            n.pointer(p)
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite)
        });
        if !n["id"].as_str().is_some_and(valid_id)
            || !kind.is_some_and(|k| !k.is_empty() && k.len() <= 32)
            || !pos_ok
        {
            return bad("nœud mal formé");
        }
    }
    for e in edges {
        if ![&e["source"], &e["target"]]
            .iter()
            .all(|v| v.as_str().is_some_and(valid_id))
        {
            return bad("lien mal formé");
        }
    }
    // Notes du canevas (facultatives) : documentation affichée, ignorée à l'exécution.
    match obj.get("notes") {
        None | Some(Value::Null) => {}
        Some(Value::Array(notes)) if notes.len() <= MAX_NOTES => {
            for n in notes {
                let text_ok = n["text"]
                    .as_str()
                    .is_some_and(|t| t.chars().count() <= MAX_NOTE_TEXT);
                if !n["id"].as_str().is_some_and(valid_id) || !text_ok {
                    return bad("note mal formée");
                }
            }
        }
        Some(_) => return bad(&format!("{MAX_NOTES} notes au maximum")),
    }
    Ok(())
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
) -> AppResult<Json<Vec<WorkflowSummary>>> {
    require_member(&state.db, ws, user.id).await?;
    Ok(Json(
        sqlx::query_as(
            "SELECT w.id, w.name, w.version, w.active, w.updated_at, u.display_name AS updated_by
             FROM workflows w LEFT JOIN users u ON u.id = w.updated_by
             WHERE w.workspace_id = $1 ORDER BY w.updated_at DESC",
        )
        .bind(ws)
        .fetch_all(&state.db)
        .await?,
    ))
}

pub(crate) async fn fetch(state: &AppState, ws: Uuid, id: Uuid) -> AppResult<WorkflowDoc> {
    sqlx::query_as("SELECT id, name, version, active, keep_data, definition, updated_at FROM workflows WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(ws)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<WorkflowDoc>> {
    require_member(&state.db, ws, user.id).await?;
    Ok(Json(fetch(&state, ws, id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBody {
    name: String,
    definition: Value,
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Json(body): Json<CreateBody>,
) -> AppResult<(StatusCode, Json<WorkflowDoc>)> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let name = validate::label(&body.name, "Nom du workflow", 120)?;
    check_definition(&body.definition)?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflows WHERE workspace_id = $1")
        .bind(ws)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_WORKFLOWS {
        return Err(AppError::bad(format!(
            "Limite de {MAX_WORKFLOWS} workflows atteinte"
        )));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflows (id, workspace_id, name, definition, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, $5)",
    )
    .bind(id)
    .bind(ws)
    .bind(name)
    .bind(sqlx::types::Json(&body.definition))
    .bind(user.id)
    .execute(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(fetch(&state, ws, id).await?)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
    name: String,
    definition: Value,
    /// Version connue du client : refus (409) si le workflow a été modifié entre-temps.
    version: i32,
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateBody>,
) -> AppResult<Json<WorkflowDoc>> {
    require_member(&state.db, ws, user.id).await?;
    let name = validate::label(&body.name, "Nom du workflow", 120)?;
    check_definition(&body.definition)?;
    let mut tx = state.db.begin().await?;
    let active: Option<bool> = sqlx::query_scalar(
        "UPDATE workflows SET name = $3, definition = $4, version = version + 1, updated_by = $5, updated_at = now()
         WHERE id = $1 AND workspace_id = $2 AND version = $6
         RETURNING active",
    )
    .bind(id)
    .bind(ws)
    .bind(name)
    .bind(sqlx::types::Json(&body.definition))
    .bind(user.id)
    .bind(body.version)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(active) = active else {
        drop(tx);
        fetch(&state, ws, id).await?; // 404 s'il n'existe pas
        return Err(AppError::Conflict(
            "Ce workflow a été modifié par quelqu'un d'autre. Rechargez-le.".into(),
        ));
    };
    // Workflow actif : ses déclencheurs suivent la nouvelle définition.
    let deactivated = if active {
        crate::triggers::resync_after_save(&mut tx, ws, id, &body.definition).await?
    } else {
        None
    };
    tx.commit().await?;
    let mut doc = fetch(&state, ws, id).await?;
    // Les autres membres qui ont ce workflow ouvert reçoivent la nouvelle version.
    state.live.saved(id, user.id, &doc);
    doc.deactivated = deactivated;
    Ok(Json(doc))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DataBody {
    keep_data: bool,
}

/// `PUT …/workflows/{id}/data` : conserver ou non les données des exécutions. Désactiver
/// efface aussi les aperçus déjà enregistrés (données personnelles).
pub async fn set_keep_data(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<DataBody>,
) -> AppResult<Json<WorkflowDoc>> {
    require_member(&state.db, ws, user.id).await?;
    let mut tx = state.db.begin().await?;
    let done =
        sqlx::query("UPDATE workflows SET keep_data = $3 WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(ws)
            .bind(body.keep_data)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if done == 0 {
        return Err(AppError::NotFound);
    }
    if !body.keep_data {
        sqlx::query(
            "UPDATE executions SET output = NULL WHERE workflow_id = $1 AND workspace_id = $2",
        )
        .bind(id)
        .bind(ws)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    tracing::info!(workflow_id = %id, by = %user.id, keep_data = body.keep_data, "conservation des données");
    let doc = fetch(&state, ws, id).await?;
    state.live.saved(id, user.id, &doc);
    Ok(Json(doc))
}

/// `DELETE …/workflows/{id}/executions` : efface l'historique du workflow (propriétaires),
/// par exemple pour une demande d'effacement de données personnelles.
pub async fn clear_executions(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    crate::workspaces::require_owner(&state.db, ws, user.id).await?;
    let deleted = sqlx::query("DELETE FROM executions WHERE workflow_id = $1 AND workspace_id = $2 AND status <> 'running'")
        .bind(id)
        .bind(ws)
        .execute(&state.db)
        .await?
        .rows_affected();
    tracing::info!(workflow_id = %id, by = %user.id, deleted, "historique effacé");
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_member(&state.db, ws, user.id).await?;
    let n = sqlx::query("DELETE FROM workflows WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(ws)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    state.live.deleted(id);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::check_definition;
    use serde_json::json;

    #[test]
    fn definitions() {
        let ok = json!({"nodes": [{"id": "a-1", "position": {"x": 0, "y": 1.5}, "data": {"kind": "csv"}}], "edges": []});
        assert!(check_definition(&ok).is_ok());
        assert!(check_definition(&json!([])).is_err());
        assert!(check_definition(&json!({"nodes": [{"id": "<script>", "position": {"x": 0, "y": 0}, "data": {"kind": "x"}}], "edges": []})).is_err());
        assert!(
            check_definition(&json!({"nodes": [], "edges": [{"source": "a", "target": "../x"}]}))
                .is_err()
        );
        let note = |id: &str, text: &str| json!({"nodes": [], "edges": [], "notes": [{"id": id, "text": text}]});
        assert!(check_definition(&note("note-1", "**Info**")).is_ok());
        assert!(check_definition(&note("../x", "")).is_err());
        assert!(check_definition(&json!({"nodes": [], "edges": [], "notes": {}})).is_err());
    }
}
