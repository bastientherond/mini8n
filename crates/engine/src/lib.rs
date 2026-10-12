//! mini8n — moteur de workflows minimaliste, compilé en WebAssembly.
//!
//! Les données circulent entre les nœuds sous forme de tables Apache Arrow
//! sérialisées en Parquet.

pub mod catalog;
pub mod data;
pub mod error;
pub mod expr;
pub mod host;
pub mod nodes;
pub mod value;
pub mod workflow;

#[cfg(target_arch = "wasm32")]
mod wasm;

/// Arrow, réexporté pour les hôtes (serveur).
pub use arrow;
pub use error::{EngineError, Result};
pub use host::{BoxFuture, Host, HttpRequest, HttpResponse, NoHost};
pub use workflow::{
    Edge, Node, NodeResult, RunOptions, RunReport, Status, TriggerEvent, Workflow, run, run_with,
};
