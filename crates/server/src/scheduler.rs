//! Ordonnanceur : déclencheurs « Planification » et « Surveillance d'API ».
//!
//! Toutes les 10 s, les déclencheurs arrivés à échéance sont réservés
//! (`FOR UPDATE SKIP LOCKED` : sûr avec plusieurs instances du serveur), leur
//! prochaine échéance est calculée, puis ils sont lancés.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{DateTime, Utc};
use mini8n_engine::arrow::array::UInt32Array;
use mini8n_engine::arrow::compute::take_record_batch;
use mini8n_engine::nodes::{Params, http};
use mini8n_engine::{TriggerEvent, data};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::types::Json;
use uuid::Uuid;

use crate::net;
use crate::runner::{Runner, Start};
use crate::schedule::Plan;

const TICK: Duration = Duration::from_secs(10);
const BATCH: i64 = 20;
/// Nombre d'identifiants mémorisés par surveillance.
const MAX_SEEN: usize = 5000;

pub fn spawn(runner: Runner) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = run_due(&runner, None).await {
                tracing::error!(%e, "ordonnanceur");
            }
        }
    });
}

#[derive(sqlx::FromRow)]
struct DueRow {
    id: Uuid,
    workflow_id: Uuid,
    kind: String,
    node_id: String,
    config: Json<Value>,
    next_run_at: Option<DateTime<Utc>>,
}

struct Due {
    id: Uuid,
    workflow_id: Uuid,
    kind: String,
    node_id: String,
    config: Map<String, Value>,
    due_at: Option<DateTime<Utc>>,
}

fn every_minutes(config: &Map<String, Value>) -> i64 {
    match config.get("every") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(1.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(1.0),
        _ => 1.0,
    }
    .max(1.0) as i64
}

/// Lance les déclencheurs arrivés à échéance (`workspace` : restreint à un workspace, pour les tests).
pub async fn run_due(runner: &Runner, workspace: Option<Uuid>) -> Result<(), sqlx::Error> {
    let now = Utc::now();
    let mut tx = runner.db().begin().await?;
    let rows: Vec<DueRow> = sqlx::query_as(
        "SELECT t.id, t.workflow_id, t.kind, t.node_id, t.config, t.next_run_at
         FROM triggers t JOIN workflows w ON w.id = t.workflow_id
         WHERE w.active AND t.next_run_at IS NOT NULL AND t.next_run_at <= now()
           AND ($2::uuid IS NULL OR t.workspace_id = $2)
         ORDER BY t.next_run_at
         LIMIT $1
         FOR UPDATE OF t SKIP LOCKED",
    )
    .bind(BATCH)
    .bind(workspace)
    .fetch_all(&mut *tx)
    .await?;
    let mut due = Vec::with_capacity(rows.len());
    for DueRow {
        id,
        workflow_id,
        kind,
        node_id,
        config: Json(config),
        next_run_at: due_at,
    } in rows
    {
        let config = config.as_object().cloned().unwrap_or_default();
        let next = match kind.as_str() {
            "schedule" => Plan::from_params(&config)
                .ok()
                .and_then(|p| p.next_after(now)),
            "poll" => Some(now + chrono::Duration::minutes(every_minutes(&config))),
            _ => None,
        };
        sqlx::query("UPDATE triggers SET next_run_at = $2 WHERE id = $1")
            .bind(id)
            .bind(next)
            .execute(&mut *tx)
            .await?;
        due.push(Due {
            id,
            workflow_id,
            kind,
            node_id,
            config,
            due_at,
        });
    }
    tx.commit().await?;

    for d in due {
        let runner = runner.clone();
        tokio::spawn(async move {
            let result = match d.kind.as_str() {
                "schedule" => fire_schedule(&runner, &d).await,
                "poll" => poll(&runner, &d).await,
                _ => Ok(()),
            };
            if let Err(e) = result {
                tracing::warn!(error = %e, trigger_id = %d.id, "déclencheur");
            }
        });
    }
    Ok(())
}

async fn load(runner: &Runner, workflow_id: Uuid) -> Result<Option<(Uuid, String, Value)>, String> {
    let row: Option<(Uuid, String, Json<Value>)> = sqlx::query_as(
        "SELECT workspace_id, name, definition FROM workflows WHERE id = $1 AND active",
    )
    .bind(workflow_id)
    .fetch_optional(runner.db())
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|(ws, name, Json(def))| (ws, name, def)))
}

async fn start(
    runner: &Runner,
    d: &Due,
    kind: &'static str,
    data: mini8n_engine::arrow::array::RecordBatch,
) -> Result<(), String> {
    let Some((workspace_id, name, definition)) = load(runner, d.workflow_id).await? else {
        return Ok(()); // désactivé entre-temps
    };
    runner
        .start(Start {
            workspace_id,
            workflow_id: d.workflow_id,
            workflow_name: name,
            definition,
            trigger_kind: kind,
            trigger: Some(TriggerEvent {
                node_id: d.node_id.clone(),
                data,
            }),
        })
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn fire_schedule(runner: &Runner, d: &Due) -> Result<(), String> {
    let event = json!({
        "declenche_le": Utc::now().to_rfc3339(),
        "planifie_pour": d.due_at.map(|t| t.to_rfc3339()),
    });
    let data = data::json_to_batch(&event).map_err(|e| e.to_string())?;
    start(runner, d, "schedule", data).await
}

/// Identifiant stable d'un élément (colonne choisie, ou élément entier).
fn item_key(item: &Value, key: &str) -> String {
    let source = if key.is_empty() {
        item.to_string()
    } else {
        item.get(key)
            .map(Value::to_string)
            .unwrap_or_else(|| item.to_string())
    };
    let digest = Sha256::digest(source.as_bytes());
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// Sélectionne les éléments jamais vus et met à jour la mémoire (les plus récents d'abord).
pub fn diff_items(
    items: &[Value],
    key: &str,
    seen: &[String],
    initialized: bool,
) -> (Vec<u32>, Vec<String>) {
    let known: HashSet<&str> = seen.iter().map(String::as_str).collect();
    let mut fresh_idx = Vec::new();
    let mut fresh_keys = Vec::new();
    let mut batch_keys = HashSet::new();
    for (i, item) in items.iter().enumerate() {
        let k = item_key(item, key);
        if !known.contains(k.as_str()) && batch_keys.insert(k.clone()) {
            if initialized {
                fresh_idx.push(i as u32);
            }
            fresh_keys.push(k);
        }
    }
    let mut memory = fresh_keys;
    memory.extend(seen.iter().cloned());
    memory.truncate(MAX_SEEN);
    (fresh_idx, memory)
}

async fn save_state(runner: &Runner, d: &Due, state: Value) -> Result<(), String> {
    sqlx::query("UPDATE triggers SET state = $2 WHERE id = $1")
        .bind(d.id)
        .bind(Json(state))
        .execute(runner.db())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

async fn poll(runner: &Runner, d: &Due) -> Result<(), String> {
    let state: Option<Json<Value>> = sqlx::query_scalar("SELECT state FROM triggers WHERE id = $1")
        .bind(d.id)
        .fetch_optional(runner.db())
        .await
        .map_err(|e| e.to_string())?;
    let state = state.map(|Json(s)| s).unwrap_or_else(|| json!({}));
    let initialized = state
        .get("initialized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let seen: Vec<String> = state
        .get("seen")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let fetched = async {
        let req = http::build_request(&Params(&d.config)).map_err(|e| e.to_string())?;
        let res = net::fetch(req, &runner.config().net_policy()).await?;
        let items_path = d
            .config
            .get("items_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        http::response_to_batch(res.status, res.body, items_path).map_err(|e| e.to_string())
    }
    .await;
    let batch = match fetched {
        Ok(b) => b,
        Err(e) => {
            let mut s = state.clone();
            s["last_error"] = json!(format!("{} : {e}", Utc::now().format("%d/%m %H:%M")));
            save_state(runner, d, s).await?;
            return Err(e);
        }
    };
    let items = match data::batch_to_json(&batch).map_err(|e| e.to_string())? {
        Value::Array(a) => a,
        _ => vec![],
    };
    let key = d
        .config
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let (fresh, memory) = diff_items(&items, key, &seen, initialized);
    save_state(runner, d, json!({ "initialized": true, "seen": memory })).await?;
    if fresh.is_empty() {
        return Ok(());
    }
    let subset = take_record_batch(&batch, &UInt32Array::from(fresh)).map_err(|e| e.to_string())?;
    start(runner, d, "poll", subset).await
}

#[cfg(test)]
mod tests {
    use super::diff_items;
    use serde_json::json;

    #[test]
    fn poll_detects_only_new_items() {
        let first = vec![json!({"id": 1}), json!({"id": 2})];
        // Premier passage : tout est mémorisé, rien n'est déclenché.
        let (fresh, seen) = diff_items(&first, "id", &[], false);
        assert!(fresh.is_empty());
        assert_eq!(seen.len(), 2);
        let second = vec![
            json!({"id": 3, "titre": "nouveau"}),
            json!({"id": 1}),
            json!({"id": 2}),
        ];
        let (fresh, seen) = diff_items(&second, "id", &seen, true);
        assert_eq!(fresh, vec![0]);
        assert_eq!(seen.len(), 3);
        // Un élément modifié mais de même identifiant n'est pas nouveau.
        let (fresh, _) = diff_items(&[json!({"id": 3, "titre": "modifié"})], "id", &seen, true);
        assert!(fresh.is_empty());
        // Sans colonne identifiante, l'élément entier est comparé.
        let (fresh, _) = diff_items(&[json!({"id": 3, "titre": "modifié"})], "", &seen, true);
        assert_eq!(fresh, vec![0]);
    }
}
