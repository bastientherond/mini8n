//! API JavaScript (wasm-bindgen).

use js_sys::{Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::error::{EngineError, Result};
use crate::host::{BoxFuture, Host};
use crate::{data, workflow};

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from(js_sys::Error::new(&e.to_string()))
}

fn engine_err(e: JsValue) -> EngineError {
    let msg = e
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"));
    EngineError::Msg(msg)
}

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Catalogue des types de nœuds (JSON).
#[wasm_bindgen]
pub fn catalog() -> String {
    serde_json::to_string(&crate::catalog::all()).unwrap_or_else(|_| "[]".into())
}

/// Hôte fourni par JavaScript (tables au format Parquet) :
/// `{ callWorkflow(id, input): Promise<Uint8Array>, runIntegration(nodeJson, inputs: Uint8Array[]): Promise<Uint8Array> }`.
struct JsHost {
    call_workflow: Option<Function>,
    run_integration: Option<Function>,
    /// `onProgress("start" | "end", nodeId, resultJson?)` (synchrone, erreurs ignorées).
    on_progress: Option<Function>,
}

impl Host for JsHost {
    fn integration<'a>(
        &'a self,
        node: &'a crate::workflow::Node,
        inputs: Vec<arrow::array::RecordBatch>,
    ) -> BoxFuture<'a, Result<arrow::array::RecordBatch>> {
        Box::pin(async move {
            let Some(f) = &self.run_integration else {
                return crate::error::fail(
                    "Ce nœud s'exécute sur le serveur (intégration indisponible ici)",
                );
            };
            let array = js_sys::Array::new();
            for batch in &inputs {
                array.push(&Uint8Array::from(data::to_parquet(batch)?.as_slice()));
            }
            let node_json = serde_json::to_string(node)?;
            let promise: Promise = f
                .call2(&JsValue::NULL, &JsValue::from_str(&node_json), &array)
                .map_err(engine_err)?
                .dyn_into()
                .map_err(engine_err)?;
            let out: Uint8Array = JsFuture::from(promise)
                .await
                .map_err(engine_err)?
                .dyn_into()
                .map_err(engine_err)?;
            data::from_parquet(out.to_vec())
        })
    }

    fn call_workflow<'a>(
        &'a self,
        workflow_id: &'a str,
        input: arrow::array::RecordBatch,
    ) -> BoxFuture<'a, Result<arrow::array::RecordBatch>> {
        Box::pin(async move {
            let Some(f) = &self.call_workflow else {
                return crate::error::fail("L'appel d'un autre workflow n'est pas disponible ici");
            };
            let bytes = data::to_parquet(&input)?;
            let promise: Promise = f
                .call2(
                    &JsValue::NULL,
                    &JsValue::from_str(workflow_id),
                    &Uint8Array::from(bytes.as_slice()),
                )
                .map_err(engine_err)?
                .dyn_into()
                .map_err(engine_err)?;
            let out: Uint8Array = JsFuture::from(promise)
                .await
                .map_err(engine_err)?
                .dyn_into()
                .map_err(engine_err)?;
            data::from_parquet(out.to_vec())
        })
    }

    fn node_started(&self, node_id: &str) {
        if let Some(f) = &self.on_progress {
            let _ = f.call2(&JsValue::NULL, &"start".into(), &node_id.into());
        }
    }

    fn node_finished(&self, node_id: &str, result: &crate::workflow::NodeResult) {
        if let (Some(f), Ok(json)) = (&self.on_progress, serde_json::to_string(result)) {
            let _ = f.call3(&JsValue::NULL, &"end".into(), &node_id.into(), &json.into());
        }
    }
}

/// Exécute un workflow (JSON). Retourne `{ report: string (JSON), data: { [nodeId]: Uint8Array (Parquet) } }`.
///
/// - `trigger` : `{ nodeId, data: Uint8Array }` pour simuler le déclencheur qui démarre l'exécution ;
/// - `host` : `{ callWorkflow }` pour le nœud « Exécuter un workflow », `{ onProgress }` pour
///   suivre l'exécution nœud par nœud.
#[wasm_bindgen(js_name = runWorkflow)]
pub async fn run_workflow(
    workflow_json: String,
    target: Option<String>,
    trigger: JsValue,
    host: JsValue,
) -> std::result::Result<JsValue, JsValue> {
    let wf: workflow::Workflow = serde_json::from_str(&workflow_json).map_err(js_err)?;

    let trigger = if trigger.is_object() {
        let node_id = Reflect::get(&trigger, &"nodeId".into())?
            .as_string()
            .ok_or_else(|| js_err("trigger.nodeId manquant"))?;
        let bytes: Uint8Array = Reflect::get(&trigger, &"data".into())?.dyn_into()?;
        let data = data::from_parquet(bytes.to_vec()).map_err(js_err)?;
        Some(workflow::TriggerEvent { node_id, data })
    } else {
        None
    };
    let host_fn = |name: &str| -> Option<Function> {
        if !host.is_object() {
            return None;
        }
        Reflect::get(&host, &name.into())
            .ok()?
            .dyn_into::<Function>()
            .ok()
    };
    let (call_workflow, run_integration, on_progress) = (
        host_fn("callWorkflow"),
        host_fn("runIntegration"),
        host_fn("onProgress"),
    );

    let opts = workflow::RunOptions {
        target: target.as_deref(),
        trigger,
    };
    let report = workflow::run_with(
        &wf,
        opts,
        &JsHost {
            call_workflow,
            run_integration,
            on_progress,
        },
    )
    .await;

    let data_obj = Object::new();
    for (id, result) in &report.results {
        if let Some(bytes) = &result.parquet {
            Reflect::set(
                &data_obj,
                &JsValue::from_str(id),
                &Uint8Array::from(bytes.as_slice()),
            )?;
        }
    }
    let out = Object::new();
    let report_json = serde_json::to_string(&report).map_err(js_err)?;
    Reflect::set(&out, &"report".into(), &JsValue::from_str(&report_json))?;
    Reflect::set(&out, &"data".into(), &data_obj)?;
    Ok(out.into())
}

/// Aperçu d'une table Parquet : `{ columns, rows, total, offset }` (JSON).
#[wasm_bindgen]
pub fn preview(
    parquet: Vec<u8>,
    offset: usize,
    limit: usize,
) -> std::result::Result<String, JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    let p = data::preview(&batch, offset, limit).map_err(js_err)?;
    serde_json::to_string(&p).map_err(js_err)
}

#[wasm_bindgen(js_name = toCsv)]
pub fn to_csv(parquet: Vec<u8>) -> std::result::Result<String, JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    data::batch_to_csv(&batch).map_err(js_err)
}

#[wasm_bindgen(js_name = toJson)]
pub fn to_json(parquet: Vec<u8>) -> std::result::Result<String, JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    let json = data::batch_to_json(&batch).map_err(js_err)?;
    serde_json::to_string_pretty(&json).map_err(js_err)
}

/// Documentation du langage d'expressions (autocomplétion de l'éditeur), en JSON.
#[wasm_bindgen(js_name = expressionDocs)]
pub fn expression_docs() -> String {
    serde_json::to_string(crate::expr::docs::DOCS).unwrap_or_else(|_| "[]".into())
}

/// Tables décodées pour les essais d'expressions (les plus récentes en fin de liste).
const TABLE_CACHE: usize = 24;

thread_local! {
    static TABLES: std::cell::RefCell<Vec<(String, arrow::array::RecordBatch)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn cached(key: &str) -> Option<arrow::array::RecordBatch> {
    TABLES.with(|t| {
        t.borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, b)| b.clone())
    })
}

/// Garde en mémoire une table Parquet sous une clé (évite de la décoder à chaque frappe).
#[wasm_bindgen(js_name = loadTable)]
pub fn load_table(key: String, parquet: Vec<u8>) -> std::result::Result<(), JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    TABLES.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|(k, _)| *k != key);
        t.push((key, batch));
        let excess = t.len().saturating_sub(TABLE_CACHE);
        t.drain(..excess);
    });
    Ok(())
}

#[wasm_bindgen(js_name = hasTable)]
pub fn has_table(key: String) -> bool {
    cached(&key).is_some()
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct TrialContext {
    /// Clé de la table en entrée.
    input: Option<String>,
    /// Nom du nœud → clé de sa table.
    nodes: std::collections::BTreeMap<String, String>,
    vars: Vec<(String, serde_json::Value)>,
    row: usize,
}

/// Position UTF-16 (JavaScript) d'une position en octets.
fn utf16_offset(src: &str, byte: usize) -> usize {
    src.get(..byte.min(src.len()))
        .map_or(0, |s| s.encode_utf16().count())
}

/// Essaie une expression (`template = false`) ou un texte `{{ … }}` sur une ligne.
/// Retourne `{ ok, value, kind, error, from, to, rows }` (JSON, positions UTF-16).
#[wasm_bindgen(js_name = tryExpression)]
pub fn try_expression(src: String, template: bool, context: String) -> String {
    let cx: TrialContext = serde_json::from_str(&context).unwrap_or_default();
    let input = cx.input.as_deref().and_then(cached);
    let lookup = |name: &str| cx.nodes.get(name).and_then(|k| cached(k));
    let mut scope = crate::expr::Scope::new(input.as_ref());
    scope.nodes = Some(&lookup);
    scope.node_names = cx.nodes.keys().cloned().collect();
    for (name, v) in &cx.vars {
        let value = match v {
            serde_json::Value::Number(n) => n
                .as_i64()
                .map(crate::value::Value::Int)
                .unwrap_or_else(|| crate::value::Value::Float(n.as_f64().unwrap_or(0.0))),
            serde_json::Value::String(s) => crate::value::Value::Str(s.clone()),
            serde_json::Value::Bool(b) => crate::value::Value::Bool(*b),
            _ => crate::value::Value::Null,
        };
        scope.vars.push((name.clone(), value));
    }
    let trial = crate::expr::trial(&src, template, &scope, cx.row);
    let mut out = serde_json::to_value(&trial).unwrap_or_default();
    if let (Some(span), Some(obj)) = (trial.span, out.as_object_mut()) {
        obj.remove("span");
        obj.insert("from".into(), utf16_offset(&src, span.start).into());
        obj.insert("to".into(), utf16_offset(&src, span.end).into());
    }
    out.to_string()
}
