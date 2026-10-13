//! Exécution des workflows côté serveur (déclencheurs automatiques, exécution à distance).
//!
//! - Pool de threads dédiés (`RUNNER_THREADS`) : le moteur (calcul Arrow) ne bloque jamais
//!   le runtime HTTP ; chaque thread a son propre runtime mono-thread.
//! - File bornée : au-delà, les nouveaux déclenchements sont refusés (429).
//! - L'hôte serveur délègue réseau (client anti-SSRF) et accès SQL au runtime principal.
//! - Délai maximal par exécution, profondeur maximale des sous-workflows,
//!   les paniques du moteur sont confinées à l'exécution concernée.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use mini8n_engine::arrow::array::RecordBatch;
use mini8n_engine::nodes::triggers;
use mini8n_engine::{
    BoxFuture, EngineError, Host, HttpRequest, HttpResponse, RunOptions, RunReport, TriggerEvent,
    Workflow, data, run_with,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Json;
use tokio::runtime::Handle;
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::net;

/// Profondeur maximale des appels de sous-workflows.
pub const MAX_DEPTH: u32 = 5;
const QUEUE_CAPACITY: usize = 200;
const PREVIEW_ROWS: usize = 20;
const RESULT_ROWS: usize = 1000;
const MAX_PREVIEW_BYTES: usize = 256 * 1024;

/// Convertit une définition stockée (format de l'éditeur) en workflow du moteur.
pub fn definition_to_engine(def: &Value) -> Result<Workflow, String> {
    let invalid = || "définition de workflow invalide".to_string();
    let nodes = def
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let edges = def
        .get("edges")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let nodes: Vec<Value> = nodes
        .iter()
        .map(|n| {
            let d = &n["data"];
            json!({
                "id": n["id"],
                "kind": d["kind"],
                "name": d.get("name").and_then(Value::as_str).unwrap_or_default(),
                "params": d.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({})),
                "disabled": d.get("disabled").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect();
    let edges: Vec<Value> = edges
        .iter()
        .map(|e| {
            let input = e
                .get("targetHandle")
                .and_then(Value::as_str)
                .and_then(|h| h.strip_prefix("in-"))
                .and_then(|i| i.parse::<usize>().ok())
                .unwrap_or(0);
            json!({ "source": e["source"], "target": e["target"], "input": input })
        })
        .collect();
    serde_json::from_value(json!({ "nodes": nodes, "edges": edges }))
        .map_err(|e| format!("{}: {e}", invalid()))
}

/// Demande d'exécution.
pub struct Start {
    pub workspace_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_name: String,
    pub definition: Value,
    /// manual | schedule | webhook | form | poll | workflow | error
    pub trigger_kind: &'static str,
    pub trigger: Option<TriggerEvent>,
    /// N'exécuter que ce nœud et ses ancêtres.
    pub target: Option<String>,
    /// Suivi de la progression (affichage en direct).
    pub progress: Option<Progress>,
    /// Simulation : les lectures de bases de données se font en session lecture seule
    /// (les nœuds d'envoi sont neutralisés par l'appelant).
    pub read_only: bool,
}

/// Progression d'une exécution : `(nœud, None)` au début, `(nœud, Some(résultat))` à la fin.
pub type Progress = Arc<dyn Fn(&str, Option<&mini8n_engine::NodeResult>) + Send + Sync>;

/// Résultat final d'une exécution.
#[derive(Debug, Clone)]
pub struct Finished {
    pub execution_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
    /// Sortie du workflow (lignes JSON, 1000 max.).
    pub output: Option<Value>,
    /// Rapport complet, avec les données produites par chaque nœud (Parquet).
    pub report: Option<Arc<RunReport>>,
}

#[derive(Clone)]
struct Ctx {
    workspace_id: Uuid,
    execution_id: Uuid,
    depth: u32,
    read_only: bool,
}

struct Job {
    workflow: Workflow,
    trigger: Option<TriggerEvent>,
    target: Option<String>,
    progress: Option<Progress>,
    ctx: Ctx,
    reply: oneshot::Sender<RunReport>,
}

struct Inner {
    db: PgPool,
    config: Arc<Config>,
    vault: Arc<crate::crypto::Vault>,
    handle: Handle,
    tx: mpsc::Sender<Job>,
}

#[derive(Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

type SendFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl Runner {
    /// Démarre le pool de threads d'exécution (à appeler depuis le runtime principal).
    pub fn start_pool(db: PgPool, config: Arc<Config>, vault: Arc<crate::crypto::Vault>) -> Runner {
        let (tx, rx) = mpsc::channel::<Job>(QUEUE_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        let inner = Arc::new(Inner {
            db,
            config: config.clone(),
            vault,
            handle: Handle::current(),
            tx,
        });
        for i in 0..config.runner_threads {
            let (inner, rx) = (inner.clone(), rx.clone());
            std::thread::Builder::new()
                .name(format!("mini8n-runner-{i}"))
                .spawn(move || worker(inner, rx))
                .expect("création d'un thread d'exécution");
        }
        Runner { inner }
    }

    /// Exécutions en attente dans la file, et capacité de la file (supervision).
    pub fn queue(&self) -> (usize, usize) {
        let free = self.inner.tx.capacity();
        (QUEUE_CAPACITY - free, QUEUE_CAPACITY)
    }

    pub fn db(&self) -> &PgPool {
        &self.inner.db
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Enregistre l'exécution et la met en file ; le récepteur renvoie le résultat final.
    pub fn start(&self, s: Start) -> SendFuture<AppResult<(Uuid, oneshot::Receiver<Finished>)>> {
        let runner = self.clone();
        Box::pin(async move { runner.start_inner(s).await })
    }

    async fn start_inner(&self, s: Start) -> AppResult<(Uuid, oneshot::Receiver<Finished>)> {
        let db = &self.inner.db;
        let id = Uuid::new_v4();
        let trigger_node = s.trigger.as_ref().map(|t| t.node_id.clone());
        record_start(
            db,
            id,
            s.workspace_id,
            Some(s.workflow_id),
            &s.workflow_name,
            s.trigger_kind,
            trigger_node.as_deref(),
            None,
        )
        .await?;
        let (done_tx, done_rx) = oneshot::channel();

        let workflow = match definition_to_engine(&s.definition) {
            Ok(w) => w,
            Err(e) => {
                let summary = Summary::error(e);
                record_finish(db, id, &summary).await?;
                let _ = done_tx.send(summary.finished(id));
                return Ok((id, done_rx));
            }
        };
        let (reply_tx, reply_rx) = oneshot::channel();
        let job = Job {
            workflow: workflow.clone(),
            trigger: s.trigger,
            target: s.target,
            progress: s.progress,
            ctx: Ctx {
                workspace_id: s.workspace_id,
                execution_id: id,
                depth: 0,
                read_only: s.read_only,
            },
            reply: reply_tx,
        };
        if self.inner.tx.try_send(job).is_err() {
            record_finish(
                db,
                id,
                &Summary::error("File d'exécution pleine, réessayez plus tard".into()),
            )
            .await?;
            return Err(AppError::TooManyRequests(10));
        }

        let runner = self.clone();
        let kind = s.trigger_kind;
        tokio::spawn(async move {
            let report = reply_rx
                .await
                .unwrap_or_else(|_| RunReport::failure("Exécution interrompue".into()));
            let summary = summarize(&workflow, &report);
            if let Err(e) = record_finish(&runner.inner.db, id, &summary).await {
                tracing::error!(%e, execution_id = %id, "enregistrement de l'exécution");
            }
            // Un échec automatique déclenche les workflows « Sur erreur » (jamais en cascade).
            if !summary.ok && kind != "error" {
                runner
                    .fire_error_triggers(
                        s.workspace_id,
                        s.workflow_id,
                        &s.workflow_name,
                        id,
                        &summary,
                    )
                    .await;
            }
            let _ = done_tx.send(Finished {
                report: Some(Arc::new(report)),
                ..summary.finished(id)
            });
        });
        Ok((id, done_rx))
    }

    async fn fire_error_triggers(
        &self,
        ws: Uuid,
        failed: Uuid,
        failed_name: &str,
        execution_id: Uuid,
        summary: &Summary,
    ) {
        let rows: Result<Vec<ErrorTarget>, _> = sqlx::query_as(
            "SELECT w.id, w.name, t.node_id, w.definition
             FROM triggers t JOIN workflows w ON w.id = t.workflow_id
             WHERE t.kind = 'error' AND t.workspace_id = $1 AND w.active AND w.id <> $2",
        )
        .bind(ws)
        .bind(failed)
        .fetch_all(&self.inner.db)
        .await;
        let rows = match rows {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(%e, "recherche des déclencheurs d'erreur");
                return;
            }
        };
        for ErrorTarget {
            id: wf_id,
            name,
            node_id,
            definition: Json(definition),
        } in rows
        {
            let event = json!({
                "workflow": failed_name,
                "workflow_id": failed.to_string(),
                "execution_id": execution_id.to_string(),
                "noeud": summary.error_node.clone().unwrap_or_default(),
                "erreur": summary.error.clone().unwrap_or_default(),
                "date": Utc::now().to_rfc3339(),
            });
            let Ok(data) = data::json_to_batch(&event) else {
                continue;
            };
            let start = Start {
                workspace_id: ws,
                workflow_id: wf_id,
                workflow_name: name,
                definition,
                trigger_kind: "error",
                trigger: Some(TriggerEvent { node_id, data }),
                target: None,
                progress: None,
                read_only: false,
            };
            if let Err(e) = self.start(start).await {
                tracing::warn!(%e, workflow_id = %wf_id, "déclencheur d'erreur non lancé");
            }
        }
    }
}

/// Workflow à lancer par un déclencheur « Sur erreur ».
#[derive(sqlx::FromRow)]
struct ErrorTarget {
    id: Uuid,
    name: String,
    node_id: String,
    definition: Json<Value>,
}

/// Marque les exécutions interrompues par un arrêt du serveur.
pub async fn recover_interrupted(db: &PgPool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE executions SET status = 'error', error = 'Interrompue (redémarrage du serveur)', finished_at = now()
         WHERE status = 'running'",
    )
    .execute(db)
    .await?
    .rows_affected())
}

/// Supprime l'historique plus ancien que la durée de rétention.
pub async fn purge_old(db: &PgPool, days: i64) -> Result<u64, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM executions WHERE started_at < now() - make_interval(days => $1)")
            .bind(days as i32)
            .execute(db)
            .await?
            .rows_affected(),
    )
}

fn worker(inner: Arc<Inner>, rx: Arc<Mutex<mpsc::Receiver<Job>>>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime d'exécution");
    let timeout = Duration::from_secs(inner.config.execution_timeout_secs);
    loop {
        let Some(job) = rt.block_on(async { rx.lock().await.recv().await }) else {
            break;
        };
        let host = ServerHost {
            inner: inner.clone(),
            ctx: job.ctx.clone(),
            progress: job.progress,
        };
        let opts = RunOptions {
            target: job.target.as_deref(),
            trigger: job.trigger,
        };
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            rt.block_on(async {
                match tokio::time::timeout(timeout, run_with(&job.workflow, opts, &host)).await {
                    Ok(report) => report,
                    Err(_) => RunReport::failure(format!(
                        "Délai d'exécution dépassé ({} s)",
                        timeout.as_secs()
                    )),
                }
            })
        }));
        let report = result.unwrap_or_else(|_| {
            tracing::error!(execution_id = %job.ctx.execution_id, "panique du moteur");
            RunReport::failure("Erreur interne du moteur".into())
        });
        let _ = job.reply.send(report);
    }
}

/// Hôte du moteur côté serveur.
struct ServerHost {
    inner: Arc<Inner>,
    ctx: Ctx,
    progress: Option<Progress>,
}

fn engine_err(msg: impl Into<String>) -> EngineError {
    EngineError::Msg(msg.into())
}

impl Host for ServerHost {
    fn node_started(&self, node_id: &str) {
        if let Some(p) = &self.progress {
            p(node_id, None);
        }
    }

    fn node_finished(&self, node_id: &str, result: &mini8n_engine::NodeResult) {
        if let Some(p) = &self.progress {
            p(node_id, Some(result));
        }
    }

    fn integration<'a>(
        &'a self,
        node: &'a mini8n_engine::Node,
        inputs: Vec<RecordBatch>,
    ) -> BoxFuture<'a, mini8n_engine::Result<RecordBatch>> {
        let ctx = crate::integrations::Ctx {
            db: self.inner.db.clone(),
            vault: self.inner.vault.clone(),
            config: self.inner.config.clone(),
            workspace_id: self.ctx.workspace_id,
            execution_id: Some(self.ctx.execution_id),
            user_id: None,
            read_only: self.ctx.read_only,
        };
        let node = node.clone();
        // Pilotes SQL, SMTP et client HTTP tournent sur le runtime principal.
        let task = self
            .inner
            .handle
            .spawn(async move { crate::integrations::run(&ctx, &node, inputs).await });
        Box::pin(async move {
            match task.await {
                Ok(res) => res.map_err(engine_err),
                Err(e) => Err(engine_err(format!("Intégration interrompue : {e}"))),
            }
        })
    }

    fn http<'a>(&'a self, req: HttpRequest) -> BoxFuture<'a, mini8n_engine::Result<HttpResponse>> {
        // Le client HTTP tourne sur le runtime principal (résolution DNS, TLS, pool de connexions).
        let task = self.inner.handle.spawn({
            let policy = self.inner.config.net_policy();
            async move { net::fetch(req, &policy).await }
        });
        Box::pin(async move {
            match task.await {
                Ok(Ok(res)) => Ok(res),
                Ok(Err(e)) => Err(engine_err(format!("Requête HTTP : {e}"))),
                Err(e) => Err(engine_err(format!("Requête HTTP interrompue : {e}"))),
            }
        })
    }

    fn call_workflow<'a>(
        &'a self,
        workflow_id: &'a str,
        input: RecordBatch,
    ) -> BoxFuture<'a, mini8n_engine::Result<RecordBatch>> {
        Box::pin(async move {
            if self.ctx.depth + 1 >= MAX_DEPTH {
                return Err(engine_err(format!(
                    "Trop d'appels imbriqués ({MAX_DEPTH} niveaux max.)"
                )));
            }
            let id = Uuid::parse_str(workflow_id.trim())
                .map_err(|_| engine_err("Identifiant de workflow invalide"))?;
            let ws = self.ctx.workspace_id;
            let db = self.inner.db.clone();
            let (name, definition) = self
                .inner
                .handle
                .spawn(async move { load_workflow(&db, ws, id).await })
                .await
                .map_err(|e| engine_err(e.to_string()))?
                .map_err(engine_err)?;
            let workflow = definition_to_engine(&definition).map_err(engine_err)?;
            let trigger_node = workflow
                .nodes
                .iter()
                .find(|n| n.kind == triggers::WORKFLOW && !n.disabled)
                .map(|n| n.id.clone())
                .ok_or_else(|| {
                    engine_err(format!(
                        "Le workflow « {name} » n'a pas de déclencheur « Appelé par un workflow »"
                    ))
                })?;

            let child = Uuid::new_v4();
            let parent = self.ctx.execution_id;
            {
                let (db, name, node) = (self.inner.db.clone(), name.clone(), trigger_node.clone());
                self.inner
                    .handle
                    .spawn(async move {
                        record_start(
                            &db,
                            child,
                            ws,
                            Some(id),
                            &name,
                            "workflow",
                            Some(&node),
                            Some(parent),
                        )
                        .await
                    })
                    .await
                    .map_err(|e| engine_err(e.to_string()))?
                    .map_err(|e| engine_err(e.to_string()))?;
            }
            let host = ServerHost {
                inner: self.inner.clone(),
                ctx: Ctx {
                    workspace_id: ws,
                    execution_id: child,
                    depth: self.ctx.depth + 1,
                    read_only: self.ctx.read_only,
                },
                progress: None,
            };
            let opts = RunOptions {
                target: None,
                trigger: Some(TriggerEvent {
                    node_id: trigger_node,
                    data: input,
                }),
            };
            let report = run_with(&workflow, opts, &host).await;
            let summary = summarize(&workflow, &report);
            {
                let (db, s) = (self.inner.db.clone(), summary.clone());
                let _ = self
                    .inner
                    .handle
                    .spawn(async move { record_finish(&db, child, &s).await })
                    .await;
            }
            if !summary.ok {
                return Err(engine_err(format!(
                    "Le workflow « {name} » a échoué : {}",
                    summary.error.unwrap_or_default()
                )));
            }
            report.output_batch()?.ok_or_else(|| {
                engine_err(format!("Le workflow « {name} » n'a produit aucune sortie"))
            })
        })
    }
}

async fn load_workflow(db: &PgPool, ws: Uuid, id: Uuid) -> Result<(String, Value), String> {
    let row: Option<(String, Json<Value>)> = sqlx::query_as(
        "SELECT name, definition FROM workflows WHERE id = $1 AND workspace_id = $2",
    )
    .bind(id)
    .bind(ws)
    .fetch_optional(db)
    .await
    .map_err(|e| e.to_string())?;
    row.map(|(n, Json(d))| (n, d))
        .ok_or_else(|| "Workflow appelé introuvable dans ce workspace".to_string())
}

/// Résumé persistant d'une exécution.
#[derive(Debug, Clone)]
struct Summary {
    ok: bool,
    error: Option<String>,
    error_node: Option<String>,
    duration_ms: f64,
    report: Value,
    preview: Option<Value>,
    output: Option<Value>,
}

impl Summary {
    fn error(msg: String) -> Self {
        Summary {
            ok: false,
            error: Some(msg),
            error_node: None,
            duration_ms: 0.0,
            report: Value::Null,
            preview: None,
            output: None,
        }
    }

    fn finished(&self, execution_id: Uuid) -> Finished {
        Finished {
            execution_id,
            ok: self.ok,
            error: self.error.clone(),
            output: self.output.clone(),
            report: None,
        }
    }
}

fn summarize(wf: &Workflow, report: &RunReport) -> Summary {
    let names: HashMap<&str, &str> = wf
        .nodes
        .iter()
        .map(|n| {
            (
                n.id.as_str(),
                if n.name.is_empty() {
                    n.kind.as_str()
                } else {
                    n.name.as_str()
                },
            )
        })
        .collect();
    let failed = report.first_error();
    let error_node = failed.map(|(id, _)| names.get(id).copied().unwrap_or(id).to_string());
    let error = report.error.clone().or_else(|| {
        failed.map(|(id, msg)| format!("{} : {msg}", names.get(id).copied().unwrap_or(id)))
    });
    let ok = report.ok && error.is_none();

    let (preview, output) = match report.output_batch() {
        Ok(Some(batch)) => {
            let preview = data::preview(&batch, 0, PREVIEW_ROWS)
                .ok()
                .and_then(|p| serde_json::to_value(p).ok())
                .filter(|v| v.to_string().len() <= MAX_PREVIEW_BYTES);
            let rows = batch.slice(0, batch.num_rows().min(RESULT_ROWS));
            (preview, data::batch_to_json(&rows).ok())
        }
        _ => (None, None),
    };
    let mut report_json = serde_json::to_value(report).unwrap_or(Value::Null);
    if let Some(obj) = report_json.as_object_mut() {
        obj.insert("names".into(), json!(names));
    }
    Summary {
        ok,
        error,
        error_node,
        duration_ms: report.duration_ms,
        report: report_json,
        preview,
        output,
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_start(
    db: &PgPool,
    id: Uuid,
    ws: Uuid,
    workflow_id: Option<Uuid>,
    name: &str,
    kind: &str,
    node: Option<&str>,
    parent: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO executions (id, workspace_id, workflow_id, workflow_name, trigger_kind, trigger_node, parent_id, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'running')",
    )
    .bind(id)
    .bind(ws)
    .bind(workflow_id)
    .bind(name)
    .bind(kind)
    .bind(node)
    .bind(parent)
    .execute(db)
    .await?;
    Ok(())
}

async fn record_finish(db: &PgPool, id: Uuid, s: &Summary) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE executions SET status = $2, error = $3, finished_at = now(), duration_ms = $4, report = $5, output = $6
         WHERE id = $1",
    )
    .bind(id)
    .bind(if s.ok { "success" } else { "error" })
    .bind(&s.error)
    .bind(s.duration_ms)
    .bind(Json(&s.report))
    .bind(s.preview.as_ref().map(Json))
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::definition_to_engine;
    use serde_json::json;

    #[test]
    fn converts_editor_definitions() {
        let def = json!({
            "version": 1,
            "nodes": [
                {"id": "a", "position": {"x": 0, "y": 0}, "data": {"kind": "trigger_webhook", "name": "Hook", "params": {"method": "POST"}}},
                {"id": "b", "position": {"x": 1, "y": 0}, "data": {"kind": "merge", "name": "Fusion", "params": null, "disabled": true}}
            ],
            "edges": [{"id": "e", "source": "a", "target": "b", "targetHandle": "in-1"}]
        });
        let wf = definition_to_engine(&def).unwrap();
        assert_eq!(wf.nodes[0].kind, "trigger_webhook");
        assert!(wf.nodes[1].disabled && wf.nodes[1].params.is_empty());
        assert_eq!(wf.edges[0].input, 1);
        assert!(definition_to_engine(&json!({"nodes": 1})).is_err());
    }
}
