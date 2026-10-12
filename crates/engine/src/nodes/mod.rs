//! Implémentation des nœuds. Chaque nœud reçoit des tables Arrow (décodées depuis
//! le Parquet produit par ses prédécesseurs) et retourne une table.

mod aggregate;
pub mod http;
mod merge;
mod sources;
mod transform;
pub mod triggers;

use arrow::array::RecordBatch;
use serde_json::{Map, Value as Json};

use crate::error::{Result, fail};
use crate::host::Host;
use crate::workflow::Node;

/// Accès typé aux paramètres d'un nœud.
pub struct Params<'a>(pub &'a Map<String, Json>);

impl Params<'_> {
    pub fn str(&self, key: &str) -> String {
        match self.0.get(key) {
            Some(Json::String(s)) => s.clone(),
            Some(Json::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    }

    pub fn str_or(&self, key: &str, default: &str) -> String {
        let s = self.str(key);
        if s.trim().is_empty() {
            default.to_string()
        } else {
            s
        }
    }

    pub fn f64(&self, key: &str, default: f64) -> f64 {
        match self.0.get(key) {
            Some(Json::Number(n)) => n.as_f64().unwrap_or(default),
            Some(Json::String(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        match self.0.get(key) {
            Some(Json::Bool(b)) => *b,
            Some(Json::String(s)) => matches!(s.as_str(), "true" | "1" | "oui"),
            _ => default,
        }
    }

    /// Liste séparée par des virgules (ou retours à la ligne).
    pub fn list(&self, key: &str) -> Vec<String> {
        split_list(&self.str(key))
    }
}

pub fn split_list(s: &str) -> Vec<String> {
    s.split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Exécute un nœud. `inputs[i]` correspond à l'entrée `i` du nœud.
pub async fn execute(
    node: &Node,
    inputs: Vec<RecordBatch>,
    host: &dyn Host,
) -> Result<RecordBatch> {
    let p = Params(&node.params);
    let mut inputs = inputs.into_iter();
    let mut input = || {
        inputs
            .next()
            .ok_or_else(|| crate::error::EngineError::Msg("Entrée manquante".into()))
    };
    match node.kind.as_str() {
        "manual" => sources::manual(&p),
        "csv" => sources::csv(&p),
        "http" => http::request(&p, host).await,
        "filter" => transform::filter(&p, input()?),
        "formula" => transform::formula(&p, input()?),
        "select" => transform::select(&p, input()?),
        "rename" => transform::rename(&p, input()?),
        "sort" => transform::sort(&p, input()?),
        "limit" => transform::limit(&p, input()?),
        "distinct" => transform::distinct(&p, input()?),
        "aggregate" => aggregate::aggregate(&p, input()?),
        "merge" => {
            let a = input()?;
            let b = input()?;
            merge::merge(&p, a, b)
        }
        "subworkflow" => {
            let id = p.str("workflow");
            if id.trim().is_empty() {
                return fail("Choisissez le workflow à exécuter");
            }
            host.call_workflow(id.trim(), input()?).await
        }
        kind if triggers::is_trigger(kind) => triggers::test_output(node, host).await,
        kind if crate::catalog::is_integration(kind) => {
            let inputs: Vec<RecordBatch> = inputs.collect();
            host.integration(node, inputs).await
        }
        other => fail(format!("Type de nœud inconnu : {other}")),
    }
}

/// Indices de colonnes à partir de leurs noms, avec message d'erreur explicite.
pub fn column_indices(batch: &RecordBatch, names: &[String]) -> Result<Vec<usize>> {
    let schema = batch.schema();
    names
        .iter()
        .map(|n| {
            schema.index_of(n).or_else(|_| {
                let avail: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();
                fail(format!(
                    "Colonne inconnue « {n} » (disponibles : {})",
                    avail.join(", ")
                ))
            })
        })
        .collect()
}
