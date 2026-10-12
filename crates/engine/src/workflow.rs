//! Modèle de workflow et exécution en graphe orienté acyclique.
//!
//! Chaque nœud produit une table sérialisée en **Parquet** ; ses successeurs
//! relisent ce Parquet. C'est le format d'échange entre nœuds (et ce que
//! l'interface télécharge / prévisualise).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Map;

use crate::catalog;
use crate::data::{self, ColumnInfo};
use crate::error::{EngineError, Result};
use crate::nodes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub params: Map<String, serde_json::Value>,
    /// Un nœud désactivé transmet sa première entrée telle quelle.
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    /// Index de l'entrée du nœud cible (0 = A, 1 = B…).
    #[serde(default)]
    pub input: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeResult {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub rows: usize,
    pub columns: Vec<ColumnInfo>,
    pub duration_ms: f64,
    /// Taille du Parquet produit (octets).
    pub bytes: usize,
    #[serde(skip)]
    pub parquet: Option<Vec<u8>>,
}

impl NodeResult {
    fn failed(status: Status, msg: impl Into<String>, duration_ms: f64) -> Self {
        Self {
            status,
            error: Some(msg.into()),
            rows: 0,
            columns: vec![],
            duration_ms,
            bytes: 0,
            parquet: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub order: Vec<String>,
    pub results: BTreeMap<String, NodeResult>,
    pub duration_ms: f64,
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

impl Workflow {
    /// Ordre topologique (stable selon l'ordre de déclaration des nœuds).
    pub fn topological_order(&self) -> Result<Vec<String>> {
        let ids: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        if ids.len() != self.nodes.len() {
            return Err(EngineError::Msg("Identifiants de nœuds en double".into()));
        }
        let mut indegree: HashMap<&str, usize> =
            self.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
        let mut succ: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &self.edges {
            if !ids.contains(e.source.as_str()) || !ids.contains(e.target.as_str()) {
                return Err(EngineError::Msg(format!(
                    "Lien vers un nœud inexistant : {} → {}",
                    e.source, e.target
                )));
            }
            *indegree.get_mut(e.target.as_str()).unwrap() += 1;
            succ.entry(e.source.as_str())
                .or_default()
                .push(e.target.as_str());
        }
        let mut queue: VecDeque<&str> = self
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .filter(|id| indegree[id] == 0)
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = queue.pop_front() {
            order.push(id.to_string());
            for &next in succ.get(id).map(Vec::as_slice).unwrap_or_default() {
                let d = indegree.get_mut(next).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(next);
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err(EngineError::Msg("Le workflow contient un cycle".into()));
        }
        Ok(order)
    }

    /// Le nœud et tous ses ancêtres.
    fn ancestors_of(&self, target: &str) -> HashSet<String> {
        let mut set = HashSet::new();
        let mut stack = vec![target.to_string()];
        while let Some(id) = stack.pop() {
            if set.insert(id.clone()) {
                stack.extend(
                    self.edges
                        .iter()
                        .filter(|e| e.target == id)
                        .map(|e| e.source.clone()),
                );
            }
        }
        set
    }
}

/// Exécute le workflow. Avec `target`, seuls ce nœud et ses ancêtres sont exécutés.
pub async fn run(wf: &Workflow, target: Option<&str>) -> RunReport {
    let start = now_ms();
    let order = match wf.topological_order() {
        Ok(o) => o,
        Err(e) => {
            return RunReport {
                ok: false,
                error: Some(e.to_string()),
                order: vec![],
                results: BTreeMap::new(),
                duration_ms: 0.0,
            };
        }
    };
    let order: Vec<String> = match target {
        Some(t) => {
            let keep = wf.ancestors_of(t);
            order.into_iter().filter(|id| keep.contains(id)).collect()
        }
        None => order,
    };
    let by_id: HashMap<&str, &Node> = wf.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut results: BTreeMap<String, NodeResult> = BTreeMap::new();

    for id in &order {
        let node = by_id[id.as_str()];
        let t0 = now_ms();
        let result = match run_node(wf, node, &results).await {
            Ok(Some(batch)) => match data::to_parquet(&batch) {
                Ok(bytes) => NodeResult {
                    status: Status::Success,
                    error: None,
                    rows: batch.num_rows(),
                    columns: data::columns(&batch.schema()),
                    duration_ms: now_ms() - t0,
                    bytes: bytes.len(),
                    parquet: Some(bytes),
                },
                Err(e) => NodeResult::failed(Status::Error, e.to_string(), now_ms() - t0),
            },
            Ok(None) => NodeResult::failed(
                Status::Skipped,
                "Une entrée en amont n'a pas produit de données",
                0.0,
            ),
            Err(e) => NodeResult::failed(Status::Error, e.to_string(), now_ms() - t0),
        };
        results.insert(id.clone(), result);
    }

    let ok = results.values().all(|r| r.status == Status::Success);
    RunReport {
        ok,
        error: None,
        order,
        results,
        duration_ms: now_ms() - start,
    }
}

/// `Ok(None)` = nœud ignoré car une entrée est indisponible.
async fn run_node(
    wf: &Workflow,
    node: &Node,
    results: &BTreeMap<String, NodeResult>,
) -> Result<Option<arrow::array::RecordBatch>> {
    let Some(spec) = catalog::spec(&node.kind) else {
        return Err(EngineError::Msg(format!(
            "Type de nœud inconnu : {}",
            node.kind
        )));
    };
    let mut inputs = Vec::with_capacity(spec.inputs.len());
    for (idx, label) in spec.inputs.iter().enumerate() {
        let sources: Vec<&Edge> = wf
            .edges
            .iter()
            .filter(|e| e.target == node.id && e.input == idx)
            .collect();
        if sources.is_empty() {
            if node.disabled {
                break;
            }
            return Err(EngineError::Msg(format!(
                "Entrée « {label} » non connectée"
            )));
        }
        // Plusieurs liens vers la même entrée : les tables sont empilées.
        let mut batches = Vec::with_capacity(sources.len());
        for e in sources {
            match results.get(&e.source).and_then(|r| r.parquet.as_ref()) {
                Some(bytes) => batches.push(data::from_parquet(bytes.clone())?),
                None => return Ok(None),
            }
        }
        inputs.push(data::append_batches(&batches)?);
    }
    if node.disabled {
        return Ok(inputs.into_iter().next());
    }
    nodes::execute(node, inputs).await.map(Some)
}
