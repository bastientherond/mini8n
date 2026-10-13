//! Déclencheurs automatiques : activation des workflows, webhooks et formulaires publics,
//! exécution à distance.

use std::collections::HashMap;
use std::time::Duration;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use mini8n_engine::nodes::{Params, http, triggers as kinds};
use mini8n_engine::{TriggerEvent, data};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::types::Json as SqlJson;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::runner::{Start, definition_to_engine};
use crate::schedule::Plan;
use crate::security::ClientIp;
use crate::workspaces::require_member;

const WEBHOOK_WAIT: Duration = Duration::from_secs(30);
const RUN_WAIT: Duration = Duration::from_secs(120);
const MAX_FIELD_CHARS: usize = 10_000;

fn valid_token(t: &str) -> bool {
    (32..=128).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn param_str(p: &Map<String, Value>, key: &str) -> String {
    match p.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn kind_name(engine_kind: &str) -> Option<&'static str> {
    Some(match engine_kind {
        kinds::MANUAL => "manual",
        kinds::SCHEDULE => "schedule",
        kinds::WEBHOOK => "webhook",
        kinds::FORM => "form",
        kinds::POLL => "poll",
        kinds::WORKFLOW => "workflow",
        kinds::ERROR => "error",
        _ => return None,
    })
}

/// Déclencheur validé, prêt à être enregistré.
#[derive(Debug)]
struct Planned {
    node_id: String,
    kind: &'static str,
    token: Option<String>,
    config: Value,
    next_run_at: Option<DateTime<Utc>>,
}

/// Valide les déclencheurs d'une définition de workflow.
fn plan_triggers(def: &Value) -> Result<Vec<Planned>, String> {
    let wf = definition_to_engine(def)?;
    let now = Utc::now();
    let mut out = Vec::new();
    for n in wf.nodes.iter().filter(|n| !n.disabled) {
        let Some(kind) = kind_name(&n.kind) else {
            continue;
        };
        let label = if n.name.is_empty() {
            kind
        } else {
            n.name.as_str()
        };
        let p = &n.params;
        let mut planned = Planned {
            node_id: n.id.clone(),
            kind,
            token: None,
            config: Value::Object(p.clone()),
            next_run_at: None,
        };
        match kind {
            "webhook" | "form" => {
                let token = param_str(p, "token");
                if !valid_token(&token) {
                    return Err(format!(
                        "« {label} » : URL non générée, régénérez-la dans l'inspecteur"
                    ));
                }
                if kind == "form" {
                    kinds::parse_form_fields(&param_str(p, "fields"))
                        .map_err(|e| format!("« {label} » : {e}"))?;
                }
                planned.token = Some(token);
            }
            "schedule" => {
                let plan = Plan::from_params(p).map_err(|e| format!("« {label} » : {e}"))?;
                planned.next_run_at = plan.next_after(now);
            }
            "poll" => {
                http::build_request(&Params(p)).map_err(|e| format!("« {label} » : {e}"))?;
                if param_str(p, "every").parse::<f64>().unwrap_or(0.0) < 1.0 {
                    return Err(format!(
                        "« {label} » : intervalle invalide (1 minute minimum)"
                    ));
                }
                // Premier passage immédiat : mémorise les éléments existants.
                planned.next_run_at = Some(now);
            }
            _ => {}
        }
        out.push(planned);
    }
    if !out.iter().any(|t| t.kind != "manual") {
        return Err("Ajoutez un déclencheur automatique (planification, webhook, formulaire, surveillance d'API, appel ou erreur) pour activer ce workflow".into());
    }
    Ok(out)
}

#[derive(sqlx::FromRow)]
struct ExistingTrigger {
    node_id: String,
    kind: String,
    config: SqlJson<Value>,
    state: SqlJson<Value>,
    next_run_at: Option<DateTime<Utc>>,
}

/// Remplace les déclencheurs enregistrés d'un workflow (dans la transaction de l'appelant).
/// L'état de la surveillance et la prochaine échéance sont conservés si la configuration est inchangée.
async fn replace_triggers(
    conn: &mut PgConnection,
    ws: Uuid,
    wf: Uuid,
    planned: Vec<Planned>,
) -> AppResult<()> {
    let previous: Vec<ExistingTrigger> = sqlx::query_as(
        "SELECT node_id, kind, config, state, next_run_at FROM triggers WHERE workflow_id = $1",
    )
    .bind(wf)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM triggers WHERE workflow_id = $1")
        .bind(wf)
        .execute(&mut *conn)
        .await?;
    for t in planned {
        let kept = previous
            .iter()
            .find(|p| p.node_id == t.node_id && p.kind == t.kind && p.config.0 == t.config);
        let (state, next_run_at) = match kept {
            Some(p) => (p.state.0.clone(), p.next_run_at.or(t.next_run_at)),
            None => (json!({}), t.next_run_at),
        };
        let res = sqlx::query(
            "INSERT INTO triggers (id, workflow_id, workspace_id, node_id, kind, token, config, state, next_run_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(Uuid::new_v4())
        .bind(wf)
        .bind(ws)
        .bind(&t.node_id)
        .bind(t.kind)
        .bind(&t.token)
        .bind(SqlJson(&t.config))
        .bind(SqlJson(&state))
        .bind(next_run_at)
        .execute(&mut *conn)
        .await;
        if let Err(sqlx::Error::Database(e)) = &res
            && e.code().as_deref() == Some("23505")
        {
            return Err(AppError::Conflict(
                "Cette URL est déjà utilisée par un autre workflow : régénérez-la dans l'inspecteur".into(),
            ));
        }
        res?;
    }
    Ok(())
}

/// Après la sauvegarde d'un workflow actif : resynchronise ses déclencheurs.
/// En cas de configuration invalide, le workflow est désactivé et le motif renvoyé.
pub async fn resync_after_save(
    conn: &mut PgConnection,
    ws: Uuid,
    wf: Uuid,
    def: &Value,
) -> AppResult<Option<String>> {
    match plan_triggers(def) {
        Ok(planned) => {
            replace_triggers(conn, ws, wf, planned).await?;
            Ok(None)
        }
        Err(reason) => {
            sqlx::query("DELETE FROM triggers WHERE workflow_id = $1")
                .bind(wf)
                .execute(&mut *conn)
                .await?;
            sqlx::query("UPDATE workflows SET active = false WHERE id = $1")
                .bind(wf)
                .execute(&mut *conn)
                .await?;
            Ok(Some(reason))
        }
    }
}

// ---------------------------------------------------------------------------
// API authentifiée
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInfo {
    node_id: String,
    kind: String,
    token: Option<String>,
    next_run_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Serialize)]
pub struct ActivationState {
    active: bool,
    triggers: Vec<TriggerInfo>,
}

async fn activation_state(db: &PgPool, wf: Uuid) -> AppResult<ActivationState> {
    let active: bool = sqlx::query_scalar("SELECT active FROM workflows WHERE id = $1")
        .bind(wf)
        .fetch_one(db)
        .await?;
    let triggers = sqlx::query_as(
        "SELECT node_id, kind, token, next_run_at, state->>'last_error' AS last_error
         FROM triggers WHERE workflow_id = $1 ORDER BY kind, node_id",
    )
    .bind(wf)
    .fetch_all(db)
    .await?;
    Ok(ActivationState { active, triggers })
}

pub async fn triggers(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<ActivationState>> {
    require_member(&state.db, ws, user.id).await?;
    ensure_workflow(&state.db, ws, id).await?;
    Ok(Json(activation_state(&state.db, id).await?))
}

async fn ensure_workflow(db: &PgPool, ws: Uuid, id: Uuid) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM workflows WHERE id = $1 AND workspace_id = $2)",
    )
    .bind(id)
    .bind(ws)
    .fetch_one(db)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveBody {
    active: bool,
}

pub async fn set_active(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ActiveBody>,
) -> AppResult<Json<ActivationState>> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let mut tx = state.db.begin().await?;
    let def: Option<SqlJson<Value>> = sqlx::query_scalar(
        "SELECT definition FROM workflows WHERE id = $1 AND workspace_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(ws)
    .fetch_optional(&mut *tx)
    .await?;
    let SqlJson(def) = def.ok_or(AppError::NotFound)?;
    if body.active {
        let planned = plan_triggers(&def).map_err(AppError::BadRequest)?;
        replace_triggers(&mut tx, ws, id, planned).await?;
    } else {
        sqlx::query("DELETE FROM triggers WHERE workflow_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE workflows SET active = $2 WHERE id = $1")
        .bind(id)
        .bind(body.active)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(workflow_id = %id, active = body.active, by = %user.id, "activation du workflow");
    state.live.publish(
        id,
        serde_json::json!({ "t": "active", "active": body.active }),
    );
    Ok(Json(activation_state(&state.db, id).await?))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    execution_id: Uuid,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Exécute le workflow sur le serveur (mode manuel : chaque déclencheur produit ses données de test).
pub async fn run_now(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<RunResult>> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let row: Option<(String, SqlJson<Value>)> = sqlx::query_as(
        "SELECT name, definition FROM workflows WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(ws)
    .fetch_optional(&state.db)
    .await?;
    let (name, SqlJson(definition)) = row.ok_or(AppError::NotFound)?;
    let (execution_id, done) = state
        .runner
        .start(Start {
            workspace_id: ws,
            workflow_id: id,
            workflow_name: name,
            definition,
            trigger_kind: "manual",
            trigger: None,
        })
        .await?;
    Ok(Json(match tokio::time::timeout(RUN_WAIT, done).await {
        Ok(Ok(f)) => RunResult {
            execution_id,
            status: if f.ok { "success" } else { "error" },
            error: f.error,
        },
        _ => RunResult {
            execution_id,
            status: "running",
            error: None,
        },
    }))
}

// ---------------------------------------------------------------------------
// Points d'entrée publics
// ---------------------------------------------------------------------------

struct PublicTrigger {
    node_id: String,
    config: Map<String, Value>,
    workflow_id: Uuid,
    workspace_id: Uuid,
    name: String,
    definition: Value,
}

#[derive(sqlx::FromRow)]
struct PublicRow {
    node_id: String,
    config: SqlJson<Value>,
    workflow_id: Uuid,
    workspace_id: Uuid,
    name: String,
    definition: SqlJson<Value>,
}

async fn find_public(db: &PgPool, token: &str, kind: &str) -> AppResult<PublicTrigger> {
    if !valid_token(token) {
        return Err(AppError::NotFound);
    }
    let row: Option<PublicRow> = sqlx::query_as(
        "SELECT t.node_id, t.config, w.id AS workflow_id, w.workspace_id, w.name, w.definition
         FROM triggers t JOIN workflows w ON w.id = t.workflow_id
         WHERE t.token = $1 AND t.kind = $2 AND w.active",
    )
    .bind(token)
    .bind(kind)
    .fetch_optional(db)
    .await?;
    let row = row.ok_or(AppError::NotFound)?;
    Ok(PublicTrigger {
        node_id: row.node_id,
        config: row.config.0.as_object().cloned().unwrap_or_default(),
        workflow_id: row.workflow_id,
        workspace_id: row.workspace_id,
        name: row.name,
        definition: row.definition.0,
    })
}

/// Comparaison en temps constant (secrets).
fn same_secret(a: &str, b: &str) -> bool {
    let (ha, hb) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    ha.iter()
        .zip(hb.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Corps de la requête → lignes JSON (objet, tableau d'objets, formulaire encodé ou texte brut).
fn webhook_rows(
    method: &Method,
    headers: &HeaderMap,
    query: HashMap<String, String>,
    body: &Bytes,
) -> AppResult<Value> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let received = json!(Utc::now().to_rfc3339());
    let mut payload = if body.is_empty() {
        Value::Object(
            query
                .into_iter()
                .map(|(k, v)| (k, Value::String(v)))
                .collect(),
        )
    } else if content_type.contains("json") {
        serde_json::from_slice(body).map_err(|e| AppError::bad(format!("JSON invalide : {e}")))?
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        Value::Object(
            url::form_urlencoded::parse(body)
                .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                .collect(),
        )
    } else {
        json!({ "body": String::from_utf8_lossy(body) })
    };
    let annotate = |v: &mut Value| {
        if let Value::Object(o) = v {
            o.insert("_methode".into(), json!(method.as_str()));
            o.insert("_recu_le".into(), received.clone());
        }
    };
    match &mut payload {
        Value::Array(items) => items.iter_mut().for_each(annotate),
        Value::Object(_) => annotate(&mut payload),
        other => {
            *other = json!({ "valeur": other.clone(), "_methode": method.as_str(), "_recu_le": received })
        }
    }
    Ok(payload)
}

/// Webhook public : `/api/hooks/<jeton>`.
pub async fn webhook(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(token): Path<String>,
    method: Method,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> AppResult<Response> {
    state.limits.public_ip.limit(&ip)?;
    state.limits.hooks.limit(&token)?;
    let t = find_public(&state.db, &token, "webhook").await?;

    let allowed = param_str(&t.config, "method");
    if !(allowed.is_empty() || allowed == "toutes" || allowed.eq_ignore_ascii_case(method.as_str()))
    {
        return Ok((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({ "error": format!("Méthode attendue : {allowed}") })),
        )
            .into_response());
    }
    let secret = param_str(&t.config, "secret");
    if !secret.is_empty() {
        let given = headers
            .get("x-mini8n-secret")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if !same_secret(given, &secret) {
            tracing::warn!(%ip, workflow_id = %t.workflow_id, "webhook : secret invalide");
            return Err(AppError::Unauthorized);
        }
    }

    let rows = webhook_rows(&method, &headers, query, &body)?;
    let data = data::json_to_batch(&rows).map_err(|e| AppError::bad(e.to_string()))?;
    let wait = param_str(&t.config, "respond") == "résultat du workflow";
    let (execution_id, done) = state
        .runner
        .start(Start {
            workspace_id: t.workspace_id,
            workflow_id: t.workflow_id,
            workflow_name: t.name,
            definition: t.definition,
            trigger_kind: "webhook",
            trigger: Some(TriggerEvent {
                node_id: t.node_id,
                data,
            }),
        })
        .await?;
    if !wait {
        return Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "executionId": execution_id })),
        )
            .into_response());
    }
    Ok(match tokio::time::timeout(WEBHOOK_WAIT, done).await {
        Ok(Ok(f)) if f.ok => {
            (StatusCode::OK, Json(f.output.unwrap_or_else(|| json!([])))).into_response()
        }
        Ok(Ok(f)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": f.error.unwrap_or_default(), "executionId": execution_id })),
        )
            .into_response(),
        _ => (
            StatusCode::ACCEPTED,
            Json(json!({ "executionId": execution_id, "status": "running" })),
        )
            .into_response(),
    })
}

#[derive(Serialize)]
pub struct PublicForm {
    title: String,
    description: String,
    fields: Vec<kinds::FormField>,
}

/// Définition d'un formulaire public : `GET /api/forms/<jeton>`.
pub async fn form(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(token): Path<String>,
) -> AppResult<Json<PublicForm>> {
    state.limits.public_ip.limit(&ip)?;
    let t = find_public(&state.db, &token, "form").await?;
    Ok(Json(PublicForm {
        title: param_str(&t.config, "title"),
        description: param_str(&t.config, "description"),
        fields: kinds::parse_form_fields(&param_str(&t.config, "fields"))
            .map_err(|e| AppError::bad(e.to_string()))?,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormSubmission {
    values: Map<String, Value>,
}

/// Valide une réponse de formulaire champ par champ.
fn validate_submission(
    fields: &[kinds::FormField],
    values: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let mut row = Map::new();
    for f in fields {
        let raw = values.get(&f.key).cloned().unwrap_or(Value::Null);
        let text = match &raw {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => String::new(),
            _ => return Err(format!("« {} » : valeur invalide", f.label)),
        };
        if text.chars().count() > MAX_FIELD_CHARS {
            return Err(format!("« {} » : texte trop long", f.label));
        }
        if text.is_empty() && f.kind != "case" {
            if f.required {
                return Err(format!("« {} » est obligatoire", f.label));
            }
            row.insert(f.key.clone(), Value::Null);
            continue;
        }
        let value = match f.kind.as_str() {
            "nombre" => text
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(|n| json!(n))
                .ok_or_else(|| format!("« {} » doit être un nombre", f.label))?,
            "case" => {
                let checked = matches!(text.as_str(), "true" | "on" | "1" | "oui");
                if f.required && !checked {
                    return Err(format!("« {} » doit être coché", f.label));
                }
                json!(checked)
            }
            "email" => {
                let ok = text.len() <= 254
                    && text.split_once('@').is_some_and(|(l, d)| {
                        !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
                    })
                    && !text.contains(char::is_whitespace);
                if !ok {
                    return Err(format!("« {} » : adresse e-mail invalide", f.label));
                }
                json!(text)
            }
            "date" => {
                chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d")
                    .map_err(|_| format!("« {} » : date invalide", f.label))?;
                json!(text)
            }
            _ => json!(text),
        };
        row.insert(f.key.clone(), value);
    }
    row.insert("soumis_le".into(), json!(Utc::now().to_rfc3339()));
    Ok(row)
}

/// Envoi d'un formulaire public : `POST /api/forms/<jeton>` (même origine exigée).
pub async fn submit_form(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(token): Path<String>,
    Json(body): Json<FormSubmission>,
) -> AppResult<Json<Value>> {
    state.limits.public_ip.limit(&ip)?;
    state.limits.hooks.limit(&token)?;
    let t = find_public(&state.db, &token, "form").await?;
    let fields = kinds::parse_form_fields(&param_str(&t.config, "fields"))
        .map_err(|e| AppError::bad(e.to_string()))?;
    let row = validate_submission(&fields, &body.values).map_err(AppError::BadRequest)?;
    let data =
        data::json_to_batch(&Value::Object(row)).map_err(|e| AppError::bad(e.to_string()))?;
    state
        .runner
        .start(Start {
            workspace_id: t.workspace_id,
            workflow_id: t.workflow_id,
            workflow_name: t.name,
            definition: t.definition,
            trigger_kind: "form",
            trigger: Some(TriggerEvent {
                node_id: t.node_id,
                data,
            }),
        })
        .await?;
    let message = param_str(&t.config, "confirmation");
    Ok(Json(
        json!({ "message": if message.is_empty() { "Merci, votre réponse a bien été envoyée.".to_string() } else { message } }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(nodes: Value) -> Value {
        json!({ "nodes": nodes, "edges": [] })
    }

    fn node(id: &str, kind: &str, params: Value) -> Value {
        json!({ "id": id, "position": {"x": 0, "y": 0}, "data": { "kind": kind, "name": id, "params": params } })
    }

    #[test]
    fn activation_requires_valid_automatic_trigger() {
        assert!(plan_triggers(&def(json!([node("m", "trigger_manual", json!({}))]))).is_err());
        let token = "a".repeat(32);
        let ok = plan_triggers(&def(json!([
            node("h", "trigger_webhook", json!({ "token": token })),
            node(
                "s",
                "trigger_schedule",
                json!({ "mode": "quotidien", "time": "07:00", "timezone": "UTC" })
            ),
        ])))
        .unwrap();
        assert_eq!(ok.len(), 2);
        assert!(ok[1].next_run_at.is_some());
        let err = plan_triggers(&def(json!([node(
            "h",
            "trigger_webhook",
            json!({ "token": "court" })
        )])))
        .unwrap_err();
        assert!(err.contains("régénérez"));
        assert!(
            plan_triggers(&def(json!([node(
                "s",
                "trigger_schedule",
                json!({ "mode": "cron", "cron": "x" })
            )])))
            .is_err()
        );
        assert!(
            plan_triggers(&def(json!([node(
                "p",
                "trigger_poll",
                json!({ "url": "ftp://x", "every": 5 })
            )])))
            .is_err()
        );
    }

    #[test]
    fn form_validation() {
        let fields = kinds::parse_form_fields(
            "Nom | texte | requis\nÂge | nombre\nE-mail | email\nAccord | case | requis",
        )
        .unwrap();
        let v = |j: Value| j.as_object().unwrap().clone();
        let row = validate_submission(
            &fields,
            &v(json!({"nom": " Ada ", "age": "36,5", "accord": true})),
        )
        .unwrap();
        assert_eq!(row["nom"], "Ada");
        assert_eq!(row["age"], 36.5);
        assert!(row["e_mail"].is_null());
        assert!(
            validate_submission(&fields, &v(json!({"age": "1", "accord": true})))
                .unwrap_err()
                .contains("obligatoire")
        );
        assert!(
            validate_submission(&fields, &v(json!({"nom": "a", "age": "x", "accord": true})))
                .is_err()
        );
        assert!(
            validate_submission(
                &fields,
                &v(json!({"nom": "a", "e_mail": "pas-un-mail", "accord": true}))
            )
            .is_err()
        );
        assert!(validate_submission(&fields, &v(json!({"nom": "a", "accord": false}))).is_err());
    }

    #[test]
    fn secrets_compare() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("", "abc"));
    }
}
