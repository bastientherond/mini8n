//! Nœuds d'intégration exécutés côté serveur : bases de données (PostgreSQL, MySQL, Oracle),
//! fichiers du workspace, e-mail (SMTP), messageries (webhooks).
//!
//! Utilisés par l'exécuteur du serveur et, pour les exécutions lancées depuis l'éditeur,
//! via `POST /api/workspaces/<ws>/integrations/run` (le navigateur n'a jamais accès aux secrets).

mod binds;
mod chat;
mod email;
mod files;
mod oracle;
mod sql;
pub mod template;

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use mini8n_engine::arrow::array::RecordBatch;
use mini8n_engine::{Node, data};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::config::Config;
use crate::crypto::Vault;
use crate::error::{AppError, AppResult};
use crate::workspaces::require_member;

/// Contexte d'exécution d'une intégration.
#[derive(Clone)]
pub struct Ctx {
    pub db: PgPool,
    pub vault: Arc<Vault>,
    pub config: Arc<Config>,
    pub workspace_id: Uuid,
    pub execution_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

impl Ctx {
    pub fn new(state: &AppState, workspace_id: Uuid, user_id: Option<Uuid>) -> Self {
        Ctx {
            db: state.db.clone(),
            vault: state.vault.clone(),
            config: state.config.clone(),
            workspace_id,
            execution_id: None,
            user_id,
        }
    }

    /// Identifiant déchiffré référencé par le paramètre `credential` du nœud.
    async fn credential(
        &self,
        params: &Map<String, Value>,
        kind: &str,
    ) -> Result<Map<String, Value>, String> {
        let id = str_param(params, "credential");
        if id.is_empty() {
            return Err("Choisissez un identifiant".into());
        }
        crate::credentials::load(&self.db, &self.vault, self.workspace_id, &id, kind).await
    }
}

pub fn str_param(p: &Map<String, Value>, key: &str) -> String {
    match p.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn num_param(p: &Map<String, Value>, key: &str, default: f64) -> f64 {
    match p.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

fn one_input(inputs: Vec<RecordBatch>) -> Result<RecordBatch, String> {
    inputs
        .into_iter()
        .next()
        .ok_or_else(|| "Entrée manquante".to_string())
}

/// Lignes JSON d'une table.
pub fn rows_of(batch: &RecordBatch) -> Result<Vec<Map<String, Value>>, String> {
    match data::batch_to_json(batch).map_err(|e| e.to_string())? {
        Value::Array(items) => Ok(items
            .into_iter()
            .map(|v| match v {
                Value::Object(o) => o,
                _ => Map::new(),
            })
            .collect()),
        _ => Ok(vec![]),
    }
}

pub fn table_of(value: Value) -> Result<RecordBatch, String> {
    data::json_to_batch(&value).map_err(|e| e.to_string())
}

/// Exécute un nœud d'intégration.
pub async fn run(ctx: &Ctx, node: &Node, inputs: Vec<RecordBatch>) -> Result<RecordBatch, String> {
    let p = &node.params;
    match node.kind.as_str() {
        "postgres_query" => sql::query(ctx, sql::Db::Postgres, p, inputs.into_iter().next()).await,
        "postgres_write" => sql::write(ctx, sql::Db::Postgres, p, one_input(inputs)?).await,
        "mysql_query" => sql::query(ctx, sql::Db::MySql, p, inputs.into_iter().next()).await,
        "mysql_write" => sql::write(ctx, sql::Db::MySql, p, one_input(inputs)?).await,
        "oracle_query" => oracle::query(ctx, p, inputs.into_iter().next()).await,
        "oracle_write" => oracle::write(ctx, p, one_input(inputs)?).await,
        "file_write" => files::write(ctx, p, one_input(inputs)?).await,
        "file_read" => files::read(ctx, p).await,
        "email_send" => email::send(ctx, p, one_input(inputs)?).await,
        "chat_message" => chat::send(ctx, p, one_input(inputs)?).await,
        other => Err(format!("Intégration inconnue : {other}")),
    }
}

/// Test de connexion d'un identifiant.
pub async fn test_credential(ctx: &Ctx, kind: &str, id: &str) -> Result<String, String> {
    let mut params = Map::new();
    params.insert("credential".into(), json!(id));
    match kind {
        "postgres" => sql::test(ctx, sql::Db::Postgres, &params).await,
        "mysql" => sql::test(ctx, sql::Db::MySql, &params).await,
        "oracle" => oracle::test(ctx, &params).await,
        "smtp" => email::test(ctx, &params).await,
        "chat_webhook" => Ok("URL enregistrée (aucun message de test n'est envoyé)".into()),
        other => Err(format!("Type inconnu : {other}")),
    }
}

// ---------------------------------------------------------------------------
// Exécution depuis l'éditeur
// ---------------------------------------------------------------------------

const MAX_INPUT_BYTES: usize = 20 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    node: Node,
    /// Tables d'entrée au format Parquet, encodées en base64.
    #[serde(default)]
    inputs: Vec<String>,
}

pub async fn run_endpoint(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Json(body): Json<RunBody>,
) -> AppResult<Response> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    if !mini8n_engine::catalog::is_integration(&body.node.kind) {
        return Err(AppError::bad("Ce nœud n'est pas une intégration"));
    }
    let mut inputs = Vec::with_capacity(body.inputs.len());
    for encoded in body.inputs.iter().take(4) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| AppError::bad("Entrée invalide"))?;
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(AppError::bad("Entrée trop volumineuse (20 Mo max.)"));
        }
        inputs.push(data::from_parquet(bytes).map_err(|e| AppError::bad(e.to_string()))?);
    }
    let ctx = Ctx::new(&state, ws, Some(user.id));
    let batch = run(&ctx, &body.node, inputs)
        .await
        .map_err(AppError::BadRequest)?;
    let bytes = data::to_parquet(&batch).map_err(AppError::internal)?;
    let mut res = (StatusCode::OK, bytes).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.apache.parquet"),
    );
    Ok(res)
}
