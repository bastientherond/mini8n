//! Outils MCP proposés aux IA : lecture des workflows, exécution sur le serveur avec les
//! données de chaque nœud, modifications diffusées en direct, historique des exécutions.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mini8n_engine::nodes::triggers;
use mini8n_engine::{RunReport, TriggerEvent, catalog, data};
use serde_json::{Map, Value, json};
use sqlx::types::Json as SqlJson;
use uuid::Uuid;

use super::{KeptRun, edit};
use crate::AppState;
use crate::runner::{Progress, Start, definition_to_engine};
use crate::tokens::{Scope, TokenAuth};
use crate::workflows;

/// Attente maximale d'une exécution dans un appel d'outil (au-delà : `get_run_result`).
const RUN_WAIT: Duration = Duration::from_secs(25);
const MAX_SAMPLE: usize = 20;
/// Durée de l'indicateur « IA » sur un nœud qu'elle vient de modifier.
const FOCUS_AFTER_EDIT: Duration = Duration::from_secs(4);
const MAX_PAGE: usize = 100;
/// Texte d'une cellule renvoyé à l'IA (au-delà : tronqué).
const MAX_CELL: usize = 500;

pub const INSTRUCTIONS: &str = "mini8n : automatisation de flux de données (workflows en graphe de nœuds ; \
chaque nœud produit une table). Pour créer : create_workflow puis add_node / connect_nodes (list_credentials \
donne les identifiants des bases et de l'e-mail). Pour déboguer : list_workflows → get_workflow → run_workflow \
(statut, colonnes, erreurs et échantillon de lignes de chaque nœud) → get_node_output pour voir plus de lignes. \
Corrigez avec update_node / add_node / connect_nodes / disconnect_nodes / delete_node : les modifications \
sont enregistrées et apparaissent en direct dans l'éditeur des membres connectés. list_node_types décrit \
chaque type de nœud et ses paramètres. Expressions : syntaxe JavaScript dans {{ … }} ($json.colonne, $now…). \
Les exécutions automatiques en échec se consultent avec list_executions / get_execution.";

/// Outils permis à un jeton « lecture » (aucune exécution, aucune modification).
const READ_TOOLS: [&str; 8] = [
    "list_workflows",
    "get_workflow",
    "list_node_types",
    "list_credentials",
    "get_run_result",
    "get_node_output",
    "list_executions",
    "get_execution",
];

/// Ce que permet la portée du jeton (rappelé à l'IA à l'initialisation).
pub fn scope_note(scope: Scope) -> &'static str {
    match scope {
        Scope::Read => {
            "Jeton « lecture » : consultation seulement (aucune exécution ni modification)."
        }
        Scope::Test => {
            "Jeton « test » : exécutions en SIMULATION (e-mails, messages, écritures en base, fichiers, \
appels HTTP autres que GET et sous-workflows neutralisés ; lectures de bases en lecture seule) ; \
seuls les workflows inactifs peuvent être modifiés."
        }
        Scope::Full => {
            "Jeton « complet » : exécutions réelles (les envois partent vraiment) et modification de tous les workflows."
        }
    }
}

/// Simulation : neutralise (désactive) les nœuds qui agissent hors de mini8n ou peuvent
/// y envoyer des données. Un nœud
/// désactivé transmet son entrée telle quelle : la suite du workflow s'exécute.
fn simulate(def: &mut Value) -> Vec<String> {
    const SENDS: [&str; 7] = [
        "email_send",
        "chat_message",
        "postgres_write",
        "mysql_write",
        "oracle_write",
        "file_write",
        "subworkflow",
    ];
    let mut neutralised = Vec::new();
    for n in def["nodes"].as_array_mut().into_iter().flatten() {
        let text = |path: &str| {
            n.pointer(path)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let (kind, method, name) = (
            text("/data/kind"),
            text("/data/params/method"),
            text("/data/name"),
        );
        // Appel HTTP : neutralisé s'il modifie quelque chose (méthode autre que GET) ou s'il
        // peut transmettre des données lues (expressions dans l'URL, les en-têtes ou le corps).
        let templated = ["url", "headers", "body"]
            .iter()
            .any(|k| text(&format!("/data/params/{k}")).contains("{{"));
        let sends = SENDS.contains(&kind.as_str())
            || (kind == "http"
                && (templated || (!method.is_empty() && !method.eq_ignore_ascii_case("GET"))));
        let disabled = n
            .pointer("/data/disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if sends && !disabled {
            n["data"]["disabled"] = json!(true);
            neutralised.push(if name.is_empty() { kind } else { name });
        }
    }
    neutralised
}

pub enum Outcome {
    Done(Value),
    Unknown,
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    read_only: bool,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": name == "delete_node", "openWorldHint": name == "run_workflow" },
    })
}

/// Outils permis par la portée du jeton.
pub fn list(scope: Scope) -> Vec<Value> {
    let mut tools = all_tools();
    if scope == Scope::Read {
        tools.retain(|t| t["name"].as_str().is_some_and(|n| READ_TOOLS.contains(&n)));
    }
    if scope == Scope::Test
        && let Some(run) = tools.iter_mut().find(|t| t["name"] == "run_workflow")
    {
        run["description"] = json!(format!(
            "SIMULATION (jeton « test ») : les nœuds d'envoi sont neutralisés. {}",
            run["description"].as_str().unwrap_or_default()
        ));
    }
    tools
}

fn all_tools() -> Vec<Value> {
    let wf = json!({ "type": "string", "description": "Identifiant du workflow (list_workflows)" });
    let node = |d: &str| json!({ "type": "string", "description": d });
    vec![
        tool(
            "list_workflows",
            "Workflows",
            "Liste les workflows du workspace.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "create_workflow",
            "Créer un workflow",
            "Crée un workflow vide (à compléter avec add_node et connect_nodes).",
            json!({ "name": { "type": "string", "description": "Nom du workflow" } }),
            &["name"],
            false,
        ),
        tool(
            "list_credentials",
            "Identifiants",
            "Identifiants enregistrés du workspace (nom, type, résumé non secret) : leur id se met dans le paramètre credential des nœuds bases de données, e-mail et messagerie. Les secrets ne sont jamais exposés.",
            json!({ "kind": { "type": "string", "description": "Filtrer par type : postgres, mysql, oracle, smtp, chat_webhook" } }),
            &[],
            true,
        ),
        tool(
            "get_workflow",
            "Détail d'un workflow",
            "Nœuds (type, paramètres, entrées reliées), liens, état d'activation et nombre de membres qui l'ont ouvert.",
            json!({ "workflow_id": wf }),
            &["workflow_id"],
            true,
        ),
        tool(
            "list_node_types",
            "Types de nœuds",
            "Catalogue des types de nœuds : description, entrées et paramètres (type, options, valeur par défaut, aide).",
            json!({ "kind": { "type": "string", "description": "Un seul type (ex. « filter ») ; tous si absent" } }),
            &[],
            true,
        ),
        tool(
            "run_workflow",
            "Exécuter",
            "Exécute le workflow sur le serveur (chaque déclencheur produit ses données de test, sauf trigger_node_id) \
             et renvoie pour chaque nœud : statut, erreur, nombre de lignes, colonnes et un échantillon. \
             L'exécution est visible en direct dans l'éditeur. Attention : les nœuds d'envoi (e-mail, messages, écritures en base) agissent réellement.",
            json!({
                "workflow_id": wf,
                "node_id": node("N'exécuter que ce nœud et ses ancêtres"),
                "trigger_node_id": node("Déclencheur à utiliser avec trigger_data (les autres déclencheurs sont ignorés)"),
                "trigger_data": { "description": "Données reçues par le déclencheur : objet ou tableau d'objets (ex. corps d'un webhook)" },
                "sample_rows": { "type": "integer", "minimum": 0, "maximum": MAX_SAMPLE, "description": "Lignes d'échantillon par nœud (3 par défaut)" },
            }),
            &["workflow_id"],
            false,
        ),
        tool(
            "get_run_result",
            "Résultat d'une exécution",
            "Résultat d'une exécution lancée par run_workflow (attend sa fin jusqu'à 25 s).",
            json!({ "execution_id": node("Identifiant renvoyé par run_workflow"), "sample_rows": { "type": "integer", "minimum": 0, "maximum": MAX_SAMPLE } }),
            &["execution_id"],
            true,
        ),
        tool(
            "get_node_output",
            "Données d'un nœud",
            "Lignes produites par un nœud lors d'une exécution lancée par run_workflow (30 min, pagination).",
            json!({
                "execution_id": node("Identifiant renvoyé par run_workflow"),
                "node_id": node("Nœud"),
                "offset": { "type": "integer", "minimum": 0 },
                "limit": { "type": "integer", "minimum": 1, "maximum": MAX_PAGE },
            }),
            &["execution_id", "node_id"],
            true,
        ),
        tool(
            "update_node",
            "Modifier un nœud",
            "Modifie les paramètres (fusionnés ; null rétablit la valeur par défaut), le nom ou l'état d'un nœud. Enregistré et diffusé en direct.",
            json!({
                "workflow_id": wf,
                "node_id": node("Nœud à modifier"),
                "params": { "type": "object", "description": "Paramètres à changer (noms : list_node_types)" },
                "name": { "type": "string" },
                "disabled": { "type": "boolean", "description": "Un nœud désactivé transmet son entrée sans la modifier" },
            }),
            &["workflow_id", "node_id"],
            false,
        ),
        tool(
            "add_node",
            "Ajouter un nœud",
            "Ajoute un nœud, relié après connect_from et/ou avant connect_to. Avec les deux, il s'insère sur le lien existant.",
            json!({
                "workflow_id": wf,
                "kind": node("Type de nœud (list_node_types)"),
                "name": { "type": "string" },
                "params": { "type": "object" },
                "connect_from": node("Nœud dont la sortie alimente le nouveau nœud"),
                "connect_to": node("Nœud alimenté par le nouveau nœud"),
                "input": { "type": "integer", "minimum": 0, "description": "Entrée de connect_to (0 ; 1 = entrée B d'une fusion)" },
            }),
            &["workflow_id", "kind"],
            false,
        ),
        tool(
            "delete_node",
            "Supprimer un nœud",
            "Supprime un nœud ; s'il est intermédiaire, ses prédécesseurs sont reliés à ses successeurs.",
            json!({ "workflow_id": wf, "node_id": node("Nœud à supprimer") }),
            &["workflow_id", "node_id"],
            false,
        ),
        tool(
            "connect_nodes",
            "Relier deux nœuds",
            "Relie la sortie de source à l'entrée input (0 par défaut) de target.",
            json!({ "workflow_id": wf, "source": node("Nœud source"), "target": node("Nœud cible"), "input": { "type": "integer", "minimum": 0 } }),
            &["workflow_id", "source", "target"],
            false,
        ),
        tool(
            "disconnect_nodes",
            "Retirer un lien",
            "Retire le lien source → target (toutes les entrées si input est absent).",
            json!({ "workflow_id": wf, "source": node("Nœud source"), "target": node("Nœud cible"), "input": { "type": "integer", "minimum": 0 } }),
            &["workflow_id", "source", "target"],
            false,
        ),
        tool(
            "list_executions",
            "Historique",
            "Dernières exécutions sur le serveur (déclencheurs automatiques, appels, exécutions des IA).",
            json!({
                "workflow_id": wf,
                "status": { "type": "string", "enum": ["running", "success", "error"] },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
            }),
            &[],
            true,
        ),
        tool(
            "get_execution",
            "Détail d'une exécution",
            "Détail d'une exécution de l'historique : erreur, résultat de chaque nœud (sans les données) et aperçu de la sortie.",
            json!({ "execution_id": node("Identifiant (list_executions)") }),
            &["execution_id"],
            true,
        ),
    ]
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Argument requis : {key}"))
}

fn opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args[key].as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn uuid(args: &Value, key: &str) -> Result<Uuid, String> {
    Uuid::parse_str(arg(args, key)?).map_err(|_| format!("{key} : identifiant invalide"))
}

fn index(args: &Value, key: &str) -> Result<Option<usize>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        v => v
            .as_u64()
            .map(|n| Some(n as usize))
            .ok_or_else(|| format!("{key} : entier positif attendu")),
    }
}

fn object<'a>(args: &'a Value, key: &str) -> Result<Option<&'a Map<String, Value>>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::Object(o) => Ok(Some(o)),
        _ => Err(format!("{key} : objet attendu")),
    }
}

fn db_err(e: impl std::fmt::Display) -> String {
    tracing::error!(%e, "mcp : erreur interne");
    "Erreur interne du serveur".into()
}

pub async fn call(
    state: &AppState,
    auth: &TokenAuth,
    name: &str,
    args: &Value,
) -> Result<Outcome, String> {
    if auth.scope == Scope::Read
        && !READ_TOOLS.contains(&name)
        && all_tools().iter().any(|t| t["name"] == name)
    {
        return Err(format!(
            "Jeton en lecture seule : « {name} » n'est pas permis."
        ));
    }
    let value = match name {
        "list_workflows" => list_workflows(state, auth).await?,
        "create_workflow" => create_workflow(state, auth, args).await?,
        "list_credentials" => list_credentials(state, auth, args).await?,
        "get_workflow" => get_workflow(state, auth, args).await?,
        "list_node_types" => node_types(args)?,
        "run_workflow" => run_workflow(state, auth, args).await?,
        "get_run_result" => run_result(state, auth, args).await?,
        "get_node_output" => node_output(state, auth, args)?,
        "update_node" => {
            let node = arg(args, "node_id")?.to_string();
            let params = object(args, "params")?;
            let (name, disabled) = (opt(args, "name"), args["disabled"].as_bool());
            save(state, auth, args, Some(&node), |def| {
                edit::update_node(def, &node, params, name, disabled)
                    .map(|_| json!({ "node_id": node }))
            })
            .await?
        }
        "add_node" => {
            let kind = arg(args, "kind")?;
            let to = opt(args, "connect_to")
                .map(|t| (t, index(args, "input").ok().flatten().unwrap_or(0)));
            index(args, "input")?;
            let params = object(args, "params")?;
            save(state, auth, args, None, |def| {
                edit::add_node(
                    def,
                    kind,
                    opt(args, "name"),
                    params,
                    opt(args, "connect_from"),
                    to,
                )
                .map(|id| json!({ "node_id": id }))
            })
            .await?
        }
        "delete_node" => {
            let node = arg(args, "node_id")?;
            save(state, auth, args, None, |def| {
                edit::delete_node(def, node).map(|_| json!({ "deleted": node }))
            })
            .await?
        }
        "connect_nodes" => {
            let (source, target) = (arg(args, "source")?, arg(args, "target")?);
            let input = index(args, "input")?.unwrap_or(0);
            save(state, auth, args, Some(target), |def| {
                edit::connect(def, source, target, input)
                    .map(|_| json!({ "connected": [source, target], "input": input }))
            })
            .await?
        }
        "disconnect_nodes" => {
            let (source, target) = (arg(args, "source")?, arg(args, "target")?);
            let input = index(args, "input")?;
            save(state, auth, args, Some(target), |def| {
                edit::disconnect(def, source, target, input)
                    .map(|_| json!({ "disconnected": [source, target] }))
            })
            .await?
        }
        "list_executions" => list_executions(state, auth, args).await?,
        "get_execution" => get_execution(state, auth, args).await?,
        _ => return Ok(Outcome::Unknown),
    };
    Ok(Outcome::Done(value))
}

// ---------------------------------------------------------------------------
// Lecture
// ---------------------------------------------------------------------------

async fn list_workflows(state: &AppState, auth: &TokenAuth) -> Result<Value, String> {
    let rows: Vec<(Uuid, String, bool, i32, chrono::DateTime<chrono::Utc>, i64)> = sqlx::query_as(
        "SELECT id, name, active, version, updated_at, jsonb_array_length(COALESCE(definition->'nodes', '[]'::jsonb))::bigint
         FROM workflows WHERE workspace_id = $1 ORDER BY updated_at DESC LIMIT 500",
    )
    .bind(auth.workspace_id)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, active, version, updated, nodes)| {
            json!({ "id": id, "name": name, "active": active, "version": version, "updatedAt": updated, "nodes": nodes, "openBy": state.live.count(id) })
        })
        .collect();
    Ok(json!({ "workflows": list }))
}

async fn create_workflow(
    state: &AppState,
    auth: &TokenAuth,
    args: &Value,
) -> Result<Value, String> {
    state
        .limits
        .writes
        .limit(&auth.user_id.to_string())
        .map_err(|e| e.to_string())?;
    let name = crate::validate::label(arg(args, "name")?, "Nom du workflow", 120)
        .map_err(|e| e.to_string())?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflows WHERE workspace_id = $1")
        .bind(auth.workspace_id)
        .fetch_one(&state.db)
        .await
        .map_err(db_err)?;
    if count >= workflows::MAX_WORKFLOWS {
        return Err(format!(
            "Limite de {} workflows atteinte",
            workflows::MAX_WORKFLOWS
        ));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflows (id, workspace_id, name, definition, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, $5)",
    )
    .bind(id)
    .bind(auth.workspace_id)
    .bind(&name)
    .bind(SqlJson(json!({ "version": 1, "nodes": [], "edges": [] })))
    .bind(auth.user_id)
    .execute(&state.db)
    .await
    .map_err(db_err)?;
    Ok(
        json!({ "workflow_id": id, "name": name, "version": 1, "editorUrl": editor_url(state, auth, id) }),
    )
}

async fn list_credentials(
    state: &AppState,
    auth: &TokenAuth,
    args: &Value,
) -> Result<Value, String> {
    let rows: Vec<(Uuid, String, String, SqlJson<Value>)> = sqlx::query_as(
        "SELECT id, name, kind, summary FROM credentials
         WHERE workspace_id = $1 AND ($2::text IS NULL OR kind = $2) ORDER BY name",
    )
    .bind(auth.workspace_id)
    .bind(opt(args, "kind"))
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, kind, SqlJson(summary))| json!({ "id": id, "name": name, "kind": kind, "summary": summary }))
        .collect();
    Ok(json!({ "credentials": list }))
}

struct Loaded {
    name: String,
    version: i32,
    active: bool,
    definition: Value,
}

async fn load(state: &AppState, auth: &TokenAuth, id: Uuid) -> Result<Loaded, String> {
    let row: Option<(String, i32, bool, SqlJson<Value>)> = sqlx::query_as(
        "SELECT name, version, active, definition FROM workflows WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(auth.workspace_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)?;
    let (name, version, active, SqlJson(definition)) =
        row.ok_or("Workflow introuvable dans ce workspace")?;
    Ok(Loaded {
        name,
        version,
        active,
        definition,
    })
}

fn editor_url(state: &AppState, auth: &TokenAuth, id: Uuid) -> Option<String> {
    let origin = state.config.public_origins.first()?;
    Some(format!("{origin}/#/w/{}/workflows/{id}", auth.workspace_id))
}

async fn get_workflow(state: &AppState, auth: &TokenAuth, args: &Value) -> Result<Value, String> {
    let id = uuid(args, "workflow_id")?;
    let wf = load(state, auth, id).await?;
    state.mcp.agent(&state.live, auth, id);
    let mut view = edit::describe(&wf.definition);
    view["id"] = json!(id);
    view["name"] = json!(wf.name);
    view["version"] = json!(wf.version);
    view["active"] = json!(wf.active);
    view["openBy"] = json!(state.live.count(id).saturating_sub(1));
    view["editorUrl"] = json!(editor_url(state, auth, id));
    Ok(view)
}

fn node_types(args: &Value) -> Result<Value, String> {
    let all = catalog::all();
    let list: Vec<_> = match opt(args, "kind") {
        Some(k) => {
            let one: Vec<_> = all.into_iter().filter(|s| s.kind == k).collect();
            if one.is_empty() {
                return Err(format!("Type de nœud inconnu : {k}"));
            }
            one
        }
        None => all,
    };
    Ok(json!({ "nodeTypes": list }))
}

// ---------------------------------------------------------------------------
// Exécution
// ---------------------------------------------------------------------------

/// Nœuds exécutés : tous, ou `target` et ses ancêtres.
fn planned(def: &Value, target: Option<&str>) -> Vec<String> {
    let ids: Vec<String> = def["nodes"]
        .as_array()
        .map(|n| {
            n.iter()
                .filter_map(|n| n["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let Some(target) = target else { return ids };
    let mut keep = HashSet::new();
    let mut stack = vec![target.to_string()];
    while let Some(id) = stack.pop() {
        if keep.insert(id.clone()) {
            for e in def["edges"].as_array().into_iter().flatten() {
                if e["target"] == id.as_str()
                    && let Some(s) = e["source"].as_str()
                {
                    stack.push(s.to_string());
                }
            }
        }
    }
    ids.into_iter().filter(|id| keep.contains(id)).collect()
}

async fn run_workflow(state: &AppState, auth: &TokenAuth, args: &Value) -> Result<Value, String> {
    let id = uuid(args, "workflow_id")?;
    let mut wf = load(state, auth, id).await?;
    // Jeton « test » : simulation (envois neutralisés, bases en lecture seule).
    let simulated = auth.scope != Scope::Full;
    let neutralised = if simulated {
        simulate(&mut wf.definition)
    } else {
        vec![]
    };
    let engine = definition_to_engine(&wf.definition)?;
    let target = opt(args, "node_id");
    if let Some(t) = target {
        edit::find(&wf.definition, t)?;
    }
    let trigger = match opt(args, "trigger_node_id") {
        Some(node) => {
            let n = engine
                .nodes
                .iter()
                .find(|n| n.id == node)
                .ok_or_else(|| format!("Nœud introuvable : {node}"))?;
            if !triggers::is_trigger(&n.kind) {
                return Err(format!("« {} » n'est pas un déclencheur", n.name));
            }
            let payload = match &args["trigger_data"] {
                Value::Null => json!([{}]),
                v @ (Value::Object(_) | Value::Array(_)) => v.clone(),
                _ => return Err("trigger_data : objet ou tableau d'objets attendu".into()),
            };
            let data = data::json_to_batch(&payload).map_err(|e| format!("trigger_data : {e}"))?;
            Some(TriggerEvent {
                node_id: node.to_string(),
                data,
            })
        }
        None if !args["trigger_data"].is_null() => {
            return Err("trigger_data nécessite trigger_node_id".into());
        }
        None => None,
    };
    let sample = index(args, "sample_rows")?.unwrap_or(3).min(MAX_SAMPLE);
    let names: HashMap<String, String> = engine
        .nodes
        .iter()
        .map(|n| {
            (
                n.id.clone(),
                if n.name.is_empty() {
                    n.kind.clone()
                } else {
                    n.name.clone()
                },
            )
        })
        .collect();

    // Exécution affichée en direct dans l'éditeur (participant « IA »).
    let agent = state.mcp.agent(&state.live, auth, id);
    if let Some(a) = agent {
        state.live.agent_focus(id, a, target.map(String::from));
    }
    let live = Arc::new(Mutex::new(
        json!({ "pending": planned(&wf.definition, target), "active": null, "results": {}, "done": false }),
    ));
    let progress: Option<Progress> = agent.map(|a| {
        let (hub, live) = (state.live.clone(), live.clone());
        Arc::new(
            move |node: &str, result: Option<&mini8n_engine::NodeResult>| {
                let mut run = live.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    None => run["active"] = json!(node),
                    Some(r) => {
                        if let Some(p) = run["pending"].as_array_mut() {
                            p.retain(|id| id != node);
                        }
                        run["results"][node] = serde_json::to_value(r).unwrap_or_default();
                        run["active"] = Value::Null;
                    }
                }
                hub.agent_run(id, a, run.clone());
            },
        ) as Progress
    });

    let (execution_id, done) = state
        .runner
        .start(Start {
            workspace_id: auth.workspace_id,
            workflow_id: id,
            workflow_name: wf.name,
            definition: wf.definition,
            trigger_kind: "manual",
            trigger,
            target: target.map(String::from),
            progress,
            read_only: simulated,
        })
        .await
        .map_err(|e| e.to_string())?;

    // Fin de l'exécution : données conservées pour get_node_output, bilan diffusé.
    {
        let state = state.clone();
        let workspace_id = auth.workspace_id;
        tokio::spawn(async move {
            let Ok(finished) = done.await else { return };
            let report = finished.report.unwrap_or_else(|| {
                Arc::new(RunReport::failure(
                    finished.error.clone().unwrap_or_default(),
                ))
            });
            if let Some(a) = agent {
                let mut run = live.lock().unwrap_or_else(|e| e.into_inner()).clone();
                run["pending"] = json!([]);
                run["active"] = Value::Null;
                run["done"] = json!(true);
                run["ok"] = json!(finished.ok);
                run["error"] = json!(
                    finished
                        .error
                        .as_deref()
                        .map(|e| e.chars().take(4000).collect::<String>())
                );
                run["durationMs"] = json!(report.duration_ms);
                state.live.agent_run(id, a, run);
                state.live.agent_blur(id, a, None);
            }
            state.mcp.keep(KeptRun {
                execution_id,
                workspace_id,
                workflow_id: id,
                at: Instant::now(),
                names,
                report,
            });
        });
    }
    let mut result = wait_result(state, auth, execution_id, sample).await?;
    if simulated {
        result["simulation"] = json!({
            "neutralised": neutralised,
            "note": "Simulation : ces nœuds ont transmis leur entrée sans rien envoyer ; les bases ont été lues en lecture seule.",
        });
    }
    Ok(result)
}

async fn wait_result(
    state: &AppState,
    auth: &TokenAuth,
    execution_id: Uuid,
    sample: usize,
) -> Result<Value, String> {
    let deadline = Instant::now() + RUN_WAIT;
    loop {
        if let Some(run) = state.mcp.run(auth.workspace_id, execution_id) {
            return Ok(summary(&run, sample));
        }
        if Instant::now() >= deadline {
            return Ok(json!({
                "executionId": execution_id,
                "status": "running",
                "hint": "Exécution toujours en cours : appelez get_run_result avec cet executionId.",
            }));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn run_result(state: &AppState, auth: &TokenAuth, args: &Value) -> Result<Value, String> {
    let execution_id = uuid(args, "execution_id")?;
    let sample = index(args, "sample_rows")?.unwrap_or(3).min(MAX_SAMPLE);
    if state.mcp.run(auth.workspace_id, execution_id).is_none() {
        let known: Option<String> =
            sqlx::query_scalar("SELECT status FROM executions WHERE id = $1 AND workspace_id = $2")
                .bind(execution_id)
                .bind(auth.workspace_id)
                .fetch_optional(&state.db)
                .await
                .map_err(db_err)?;
        match known.as_deref() {
            None => return Err("Exécution introuvable".into()),
            Some("running") => {}
            // Exécution ancienne ou automatique : seul l'historique est disponible.
            Some(_) => return get_execution(state, auth, args).await,
        }
    }
    wait_result(state, auth, execution_id, sample).await
}

/// Raccourcit les textes longs (les cellules volumineuses restent lisibles pour l'IA).
fn shorten(v: Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > MAX_CELL => Value::String(format!(
            "{}… ({} caractères)",
            s.chars().take(MAX_CELL).collect::<String>(),
            s.chars().count()
        )),
        Value::Array(a) => Value::Array(a.into_iter().map(shorten).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, shorten(v))).collect()),
        other => other,
    }
}

fn rows_of(run: &KeptRun, node: &str, offset: usize, limit: usize) -> Option<(usize, Value)> {
    let bytes = run.report.results.get(node)?.parquet.clone()?;
    let batch = data::from_parquet(bytes).ok()?;
    let total = batch.num_rows();
    let start = offset.min(total);
    let page = batch.slice(start, limit.min(total - start));
    Some((total, shorten(data::batch_to_json(&page).ok()?)))
}

fn summary(run: &KeptRun, sample: usize) -> Value {
    let r = &run.report;
    let nodes: Vec<Value> = r
        .order
        .iter()
        .filter_map(|id| {
            let res = r.results.get(id)?;
            let mut v = json!({
                "id": id,
                "name": run.names.get(id),
                "status": res.status,
                "rows": res.rows,
                "columns": res.columns.iter().map(|c| format!("{}: {}", c.name, c.data_type)).collect::<Vec<_>>(),
                "durationMs": (res.duration_ms * 10.0).round() / 10.0,
            });
            if let Some(e) = &res.error {
                v["error"] = json!(e);
            }
            if sample > 0
                && let Some((_, rows)) = rows_of(run, id, 0, sample)
            {
                v["sample"] = rows;
            }
            Some(v)
        })
        .collect();
    json!({
        "executionId": run.execution_id,
        "workflowId": run.workflow_id,
        "status": if r.ok { "success" } else { "error" },
        "error": r.error.clone().or_else(|| r.first_error().map(|(id, e)| format!("{} : {e}", run.names.get(id).map_or(id, String::as_str)))),
        "durationMs": r.duration_ms.round(),
        "outputNode": r.output,
        "nodes": nodes,
    })
}

fn node_output(state: &AppState, auth: &TokenAuth, args: &Value) -> Result<Value, String> {
    let execution_id = uuid(args, "execution_id")?;
    let node = arg(args, "node_id")?;
    let run = state
        .mcp
        .run(auth.workspace_id, execution_id)
        .ok_or("Exécution inconnue ou expirée (30 min) : relancez run_workflow")?;
    let res = run
        .report
        .results
        .get(node)
        .ok_or_else(|| format!("Nœud non exécuté : {node}"))?;
    let offset = index(args, "offset")?.unwrap_or(0);
    let limit = index(args, "limit")?.unwrap_or(20).clamp(1, MAX_PAGE);
    let (total, rows) = rows_of(&run, node, offset, limit).ok_or_else(|| {
        format!(
            "Aucune donnée pour ce nœud ({})",
            res.error.as_deref().unwrap_or("statut : ignoré")
        )
    })?;
    Ok(
        json!({ "node": node, "name": run.names.get(node), "totalRows": total, "offset": offset, "rows": rows }),
    )
}

// ---------------------------------------------------------------------------
// Modifications
// ---------------------------------------------------------------------------

/// Le workflow peut-il être exécuté par un autre (déclencheur « Appelé par un workflow ») ?
fn callable(def: &Value) -> bool {
    def["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|n| n.pointer("/data/kind").and_then(Value::as_str) == Some(triggers::WORKFLOW))
}

/// Applique `change` à la définition et l'enregistre (nouvelle version diffusée en direct).
/// En cas d'enregistrement simultané par un membre, la modification est réappliquée.
async fn save<F>(
    state: &AppState,
    auth: &TokenAuth,
    args: &Value,
    focus: Option<&str>,
    change: F,
) -> Result<Value, String>
where
    F: Fn(&mut Value) -> Result<Value, String>,
{
    state
        .limits
        .writes
        .limit(&auth.user_id.to_string())
        .map_err(|e| e.to_string())?;
    let id = uuid(args, "workflow_id")?;
    for _ in 0..3 {
        let wf = load(state, auth, id).await?;
        let full = auth.scope == Scope::Full;
        // Un workflow actif tourne tout seul (planification, webhooks…) ; un workflow appelable
        // (« Appelé par un workflow ») s'exécute pour de vrai depuis un workflow actif. Seul un
        // jeton « complet » peut changer ce qui s'exécutera réellement.
        if !full && wf.active {
            return Err(format!(
                "« {} » est actif : désactivez-le pour le modifier, ou utilisez un jeton « complet ».",
                wf.name
            ));
        }
        let mut def = wf.definition.clone();
        let result = change(&mut def)?;
        if !full && (callable(&wf.definition) || callable(&def)) {
            return Err(format!(
                "« {} » peut être appelé par d'autres workflows (déclencheur « Appelé par un workflow ») : \
                 seul un jeton « complet » peut le modifier.",
                wf.name
            ));
        }
        workflows::check_definition(&def).map_err(|e| e.to_string())?;
        let mut tx = state.db.begin().await.map_err(db_err)?;
        // Vérification de l'état actif dans la même écriture : une activation simultanée
        // ne peut pas s'intercaler entre la lecture et l'enregistrement.
        let saved: Option<(i32, bool)> = sqlx::query_as(
            "UPDATE workflows SET definition = $3, version = version + 1, updated_by = $4, updated_at = now()
             WHERE id = $1 AND workspace_id = $2 AND version = $5 AND ($6 OR NOT active)
             RETURNING version, active",
        )
        .bind(id)
        .bind(auth.workspace_id)
        .bind(SqlJson(&def))
        .bind(auth.user_id)
        .bind(wf.version)
        .bind(full)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let Some((version, active)) = saved else {
            continue;
        };
        let deactivated = if active {
            crate::triggers::resync_after_save(&mut tx, auth.workspace_id, id, &def)
                .await
                .map_err(|e| e.to_string())?
        } else {
            None
        };
        tx.commit().await.map_err(db_err)?;
        let doc = workflows::fetch(state, auth.workspace_id, id)
            .await
            .map_err(|e| e.to_string())?;
        state.live.saved(id, auth.user_id, &doc);
        if let Some(a) = state.mcp.agent(&state.live, auth, id) {
            let node = focus
                .map(String::from)
                .or_else(|| result["node_id"].as_str().map(String::from));
            state.live.agent_focus(id, a, node.clone());
            // Indicateur visible quelques secondes, le temps de repérer la modification.
            if let Some(node) = node {
                let hub = state.live.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(FOCUS_AFTER_EDIT).await;
                    hub.agent_blur(id, a, Some(&node));
                });
            }
        }
        let mut out = json!({ "ok": true, "version": version });
        if let Value::Object(extra) = result {
            out.as_object_mut().expect("objet").extend(extra);
        }
        if let Some(reason) = deactivated {
            out["deactivated"] = json!(reason);
        }
        return Ok(out);
    }
    Err("Le workflow est modifié en continu par un membre : réessayez".into())
}

// ---------------------------------------------------------------------------
// Historique
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct ExecutionRow {
    id: Uuid,
    workflow_id: Option<Uuid>,
    workflow_name: String,
    trigger_kind: String,
    trigger_node: Option<String>,
    status: String,
    error: Option<String>,
    started_at: chrono::DateTime<chrono::Utc>,
    duration_ms: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct ExecutionDetail {
    workflow_id: Option<Uuid>,
    workflow_name: String,
    trigger_kind: String,
    status: String,
    error: Option<String>,
    report: Option<SqlJson<Value>>,
    output: Option<SqlJson<Value>>,
}

async fn list_executions(
    state: &AppState,
    auth: &TokenAuth,
    args: &Value,
) -> Result<Value, String> {
    let workflow = match opt(args, "workflow_id") {
        Some(_) => Some(uuid(args, "workflow_id")?),
        None => None,
    };
    let status = opt(args, "status").filter(|s| matches!(*s, "running" | "success" | "error"));
    let limit = index(args, "limit")?.unwrap_or(20).clamp(1, 100) as i64;
    let list: Vec<ExecutionRow> = sqlx::query_as(
        "SELECT id, workflow_id, workflow_name, trigger_kind, trigger_node, status, error, started_at, duration_ms
         FROM executions
         WHERE workspace_id = $1 AND ($2::uuid IS NULL OR workflow_id = $2) AND ($3::text IS NULL OR status = $3)
         ORDER BY started_at DESC LIMIT $4",
    )
    .bind(auth.workspace_id)
    .bind(workflow)
    .bind(status)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(db_err)?;
    Ok(json!({ "executions": list }))
}

async fn get_execution(state: &AppState, auth: &TokenAuth, args: &Value) -> Result<Value, String> {
    let id = uuid(args, "execution_id")?;
    let row: Option<ExecutionDetail> = sqlx::query_as(
        "SELECT workflow_id, workflow_name, trigger_kind, status, error, report, output FROM executions WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(auth.workspace_id)
    .fetch_optional(&state.db)
    .await
    .map_err(db_err)?;
    let ExecutionDetail {
        workflow_id,
        workflow_name: name,
        trigger_kind: kind,
        status,
        error,
        report,
        output,
    } = row.ok_or("Exécution introuvable")?;
    let output = output.map(|SqlJson(v)| match v {
        Value::Array(rows) => shorten(Value::Array(rows.into_iter().take(10).collect())),
        other => shorten(other),
    });
    Ok(json!({
        "executionId": id,
        "workflowId": workflow_id,
        "workflowName": name,
        "trigger": kind,
        "status": status,
        "error": error,
        "report": report.map(|SqlJson(v)| v),
        "outputPreview": output,
        "hint": "Les données de chaque nœud ne sont pas conservées dans l'historique : relancez avec run_workflow (trigger_node_id + trigger_data pour rejouer les données reçues).",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_described() {
        let tools = list(Scope::Full);
        let names: HashSet<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names.len(), tools.len(), "noms uniques");
        for t in &tools {
            assert_eq!(t["inputSchema"]["type"], "object");
            for r in t["inputSchema"]["required"].as_array().unwrap() {
                assert!(
                    t["inputSchema"]["properties"]
                        .get(r.as_str().unwrap())
                        .is_some(),
                    "{t}"
                );
            }
        }
        let def = json!({
            "nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
            "edges": [{"source": "a", "target": "b"}]
        });
        assert_eq!(planned(&def, Some("b")), ["a", "b"]);

        let mut sim = json!({"nodes": [
            {"id": "m", "data": {"kind": "email_send", "name": "Envoi", "params": {}}},
            {"id": "g", "data": {"kind": "http", "name": "Lecture", "params": {"method": "GET", "url": "https://api.exemple.test/x"}}},
            {"id": "x", "data": {"kind": "http", "name": "Fuite", "params": {"method": "GET", "url": "https://autre.test/?d={{ $json.email }}"}}},
            {"id": "p", "data": {"kind": "http", "name": "Écriture", "params": {"method": "POST", "url": "https://api.exemple.test/x"}}},
            {"id": "f", "data": {"kind": "filter", "name": "Filtre", "params": {}}}
        ]});
        assert_eq!(simulate(&mut sim), ["Envoi", "Fuite", "Écriture"]);
        assert_eq!(
            sim["nodes"][1]["data"].get("disabled"),
            None,
            "GET statique conservé"
        );
        assert_eq!(planned(&def, None).len(), 3);
        assert_eq!(
            shorten(json!("x".repeat(600)))
                .as_str()
                .unwrap()
                .chars()
                .count(),
            MAX_CELL + "… (600 caractères)".chars().count()
        );
    }
}
