//! API JavaScript (wasm-bindgen).

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::{data, workflow};

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from(js_sys::Error::new(&e.to_string()))
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

/// Exécute un workflow (JSON). Retourne `{ report: string (JSON), data: { [nodeId]: Uint8Array (Parquet) } }`.
#[wasm_bindgen(js_name = runWorkflow)]
pub async fn run_workflow(
    workflow_json: String,
    target: Option<String>,
) -> Result<JsValue, JsValue> {
    let wf: workflow::Workflow = serde_json::from_str(&workflow_json).map_err(js_err)?;
    let report = workflow::run(&wf, target.as_deref()).await;

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
pub fn preview(parquet: Vec<u8>, offset: usize, limit: usize) -> Result<String, JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    let p = data::preview(&batch, offset, limit).map_err(js_err)?;
    serde_json::to_string(&p).map_err(js_err)
}

#[wasm_bindgen(js_name = toCsv)]
pub fn to_csv(parquet: Vec<u8>) -> Result<String, JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    data::batch_to_csv(&batch).map_err(js_err)
}

#[wasm_bindgen(js_name = toJson)]
pub fn to_json(parquet: Vec<u8>) -> Result<String, JsValue> {
    let batch = data::from_parquet(parquet).map_err(js_err)?;
    let json = data::batch_to_json(&batch).map_err(js_err)?;
    serde_json::to_string_pretty(&json).map_err(js_err)
}
