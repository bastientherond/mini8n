//! mini8n — moteur de workflows minimaliste, compilé en WebAssembly.
//!
//! Les données circulent entre les nœuds sous forme de tables Apache Arrow
//! sérialisées en Parquet.

pub mod catalog;
pub mod data;
pub mod error;
pub mod expr;
pub mod nodes;
pub mod value;
pub mod workflow;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use error::{EngineError, Result};
pub use workflow::{Edge, Node, NodeResult, RunReport, Status, Workflow, run};
