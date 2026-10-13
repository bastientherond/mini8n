//! Serveur MCP (Model Context Protocol) : une IA — Claude Code, Claude Desktop… — se
//! connecte en direct aux workflows d'un workspace pour les examiner, les exécuter et les
//! corriger (débogage).
//!
//! - Transport « Streamable HTTP » sans session : `POST /api/mcp`, un message JSON-RPC
//!   par requête, réponse JSON. Pas de flux SSE (`GET` → 405).
//! - Authentification : jeton d'accès personnel (`Authorization: Bearer m8n_…`), limité à
//!   un workspace. Les cookies ne sont jamais utilisés (pas de CSRF possible).
//! - En direct : l'IA apparaît dans l'éditeur comme un participant (nœud examiné,
//!   exécutions) et ses modifications sont diffusées aux membres connectés.

mod edit;
mod tools;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use mini8n_engine::RunReport;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::AppState;
use crate::error::AppError;
use crate::live::Hub;
use crate::tokens::{self, TokenAuth};

/// Versions du protocole acceptées (la plus récente en premier).
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY: usize = 1024 * 1024;
/// Exécutions conservées en mémoire (données de chaque nœud) : au total, par workspace,
/// et taille maximale des données.
const KEPT_RUNS: usize = 50;
const KEPT_RUNS_PER_WORKSPACE: usize = 5;
const KEPT_BYTES: usize = 256 * 1024 * 1024;
const RUN_TTL: Duration = Duration::from_secs(30 * 60);
/// Une IA inactive quitte l'éditeur au bout de ce délai.
const AGENT_IDLE: Duration = Duration::from_secs(90);

/// Exécution lancée par une IA, conservée pour consulter les données de chaque nœud.
impl KeptRun {
    /// Taille des données conservées (Parquet de chaque nœud).
    fn bytes(&self) -> usize {
        self.report
            .results
            .values()
            .filter_map(|r| r.parquet.as_ref())
            .map(Vec::len)
            .sum()
    }
}

pub struct KeptRun {
    pub execution_id: Uuid,
    pub workspace_id: Uuid,
    pub workflow_id: Uuid,
    pub at: Instant,
    pub names: HashMap<String, String>,
    pub report: Arc<RunReport>,
}

/// État en mémoire du serveur MCP.
#[derive(Default)]
pub struct McpState {
    runs: Mutex<VecDeque<Arc<KeptRun>>>,
    /// Présence des IA dans l'éditeur : (jeton, workflow) → (participant, dernière action).
    agents: Mutex<HashMap<(Uuid, Uuid), (u64, Instant)>>,
}

impl McpState {
    /// Conserve une exécution : par workspace (un workspace n'évince pas les autres) et dans
    /// une enveloppe mémoire globale ; les plus anciennes partent en premier.
    fn keep(&self, run: KeptRun) {
        let mut runs = self.runs.lock().unwrap_or_else(|e| e.into_inner());
        runs.retain(|r| r.at.elapsed() < RUN_TTL);
        let ws = run.workspace_id;
        while runs.iter().filter(|r| r.workspace_id == ws).count() >= KEPT_RUNS_PER_WORKSPACE {
            let oldest = runs
                .iter()
                .position(|r| r.workspace_id == ws)
                .expect("présente");
            runs.remove(oldest);
        }
        runs.push_back(Arc::new(run));
        while runs.len() > KEPT_RUNS || runs.iter().map(|r| r.bytes()).sum::<usize>() > KEPT_BYTES {
            if runs.pop_front().is_none() {
                break;
            }
        }
    }

    fn run(&self, workspace: Uuid, execution: Uuid) -> Option<Arc<KeptRun>> {
        self.runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|r| r.execution_id == execution && r.workspace_id == workspace)
            .cloned()
    }

    /// Participant représentant l'IA dans l'éditeur du workflow (rejoint au besoin).
    fn agent(&self, hub: &Hub, auth: &TokenAuth, workflow: Uuid) -> Option<u64> {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        let key = (auth.token_id, workflow);
        if let Some((id, seen)) = agents.get_mut(&key)
            && hub.agent_present(workflow, *id)
        {
            *seen = Instant::now();
            return Some(*id);
        }
        let name = format!("{} · IA de {}", auth.name, auth.user_name);
        let id = hub.agent_join(auth.workspace_id, workflow, auth.user_id, &name)?;
        agents.insert(key, (id, Instant::now()));
        Some(id)
    }

    /// Jeton révoqué : ses participants quittent l'éditeur.
    pub fn forget_token(&self, hub: &Hub, token: Uuid) {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        agents.retain(|(t, wf), (id, _)| {
            if *t == token {
                hub.agent_leave(*wf, *id);
            }
            *t != token
        });
    }

    fn sweep(&self, hub: &Hub) {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        agents.retain(|(_, wf), (id, seen)| {
            let keep = seen.elapsed() < AGENT_IDLE;
            if !keep {
                hub.agent_leave(*wf, *id);
            }
            keep
        });
    }
}

/// Retire régulièrement de l'éditeur les IA inactives.
pub fn spawn_sweeper(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            state.mcp.sweep(&state.live);
        }
    });
}

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

fn json_response(status: StatusCode, body: Value) -> Response {
    let mut res = (status, body.to_string()).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}

fn unauthorized() -> Response {
    let mut res = json_response(
        StatusCode::UNAUTHORIZED,
        rpc_error(
            &Value::Null,
            -32001,
            "Jeton d'accès requis : Authorization: Bearer m8n_… (Paramètres → Accès des IA)",
        ),
    );
    res.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"mini8n\""),
    );
    res
}

/// `GET /api/mcp` : pas de flux d'événements serveur.
pub async fn stream() -> Response {
    let mut res = StatusCode::METHOD_NOT_ALLOWED.into_response();
    res.headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    res
}

/// `POST /api/mcp` : un message JSON-RPC.
pub async fn endpoint(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    // Protection contre le « DNS rebinding » : un navigateur envoie toujours Origin.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && !state.config.public_origins.iter().any(|o| o == origin)
    {
        return AppError::Forbidden("Origine non autorisée".into()).into_response();
    }
    let auth = match tokens::authenticate(&state, &headers).await {
        Ok(a) => a,
        Err(AppError::Unauthorized) => return unauthorized(),
        Err(e) => return e.into_response(),
    };
    if let Err(e) = state.limits.mcp.limit(&auth.token_id.to_string()) {
        return e.into_response();
    }
    if body.len() > MAX_BODY {
        return json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            rpc_error(&Value::Null, -32600, "Message trop volumineux"),
        );
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            rpc_error(&Value::Null, -32700, "JSON invalide"),
        );
    };
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // Réponse du client ou lot de messages : rien à traiter.
        return StatusCode::ACCEPTED.into_response();
    };
    // Notification (sans identifiant) : accusé de réception sans corps.
    let Some(id) = msg.get("id").filter(|v| v.is_string() || v.is_number()) else {
        return StatusCode::ACCEPTED.into_response();
    };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let reply = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            rpc_result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "mini8n", "title": "mini8n", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": format!("{}\n{}", tools::INSTRUCTIONS, tools::scope_note(auth.scope)),
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools::list(auth.scope) })),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let args = params
                .get("arguments")
                .filter(|a| a.is_object())
                .cloned()
                .unwrap_or(json!({}));
            match tools::call(&state, &auth, name, &args).await {
                Ok(tools::Outcome::Unknown) => {
                    rpc_error(id, -32602, format!("Outil inconnu : {name}"))
                }
                Ok(tools::Outcome::Done(value)) => rpc_result(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
                        "structuredContent": value,
                        "isError": false,
                    }),
                ),
                // Erreur métier : renvoyée à l'IA pour qu'elle corrige sa demande.
                Err(message) => rpc_result(
                    id,
                    json!({ "content": [{ "type": "text", "text": message }], "isError": true }),
                ),
            }
        }
        other => rpc_error(id, -32601, format!("Méthode inconnue : {other}")),
    };
    json_response(StatusCode::OK, reply)
}
