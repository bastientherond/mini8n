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

/// Hôte fourni par JavaScript : `{ callWorkflow(id: string, input: Uint8Array): Promise<Uint8Array> }`
/// (tables au format Parquet).
struct JsHost {
    call_workflow: Option<Function>,
}

impl Host for JsHost {
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
}

/// Exécute un workflow (JSON). Retourne `{ report: string (JSON), data: { [nodeId]: Uint8Array (Parquet) } }`.
///
/// - `trigger` : `{ nodeId, data: Uint8Array }` pour simuler le déclencheur qui démarre l'exécution ;
/// - `host` : `{ callWorkflow }` pour le nœud « Exécuter un workflow ».
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
    let call_workflow = if host.is_object() {
        Reflect::get(&host, &"callWorkflow".into())?
            .dyn_into::<Function>()
            .ok()
    } else {
        None
    };

    let opts = workflow::RunOptions {
        target: target.as_deref(),
        trigger,
    };
    let report = workflow::run_with(&wf, opts, &JsHost { call_workflow }).await;

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
