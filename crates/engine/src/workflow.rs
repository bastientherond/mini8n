//! Modèle de workflow et exécution en graphe orienté acyclique.
//!
//! Chaque nœud produit une table sérialisée en **Parquet** ; ses successeurs
//! relisent ce Parquet. C'est le format d'échange entre nœuds (et ce que
//! l'interface télécharge / prévisualise).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Map;

use arrow::array::RecordBatch;

use crate::catalog;
use crate::data::{self, ColumnInfo};
use crate::error::{EngineError, Result};
use crate::host::{Host, NoHost, now_ms};
use crate::nodes::{self, triggers};

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
    /// Nœud de sortie : dernier nœud terminal (sans successeur) exécuté avec succès.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

impl RunReport {
    /// Rapport d'échec global (workflow invalide, délai dépassé…).
    pub fn failure(msg: String) -> Self {
        Self {
            ok: false,
            error: Some(msg),
            order: vec![],
            results: BTreeMap::new(),
            duration_ms: 0.0,
            output: None,
        }
    }

    /// Table produite par le nœud de sortie.
    pub fn output_batch(&self) -> Result<Option<RecordBatch>> {
        match self
            .output
            .as_ref()
            .and_then(|id| self.results.get(id))
            .and_then(|r| r.parquet.as_ref())
        {
            Some(bytes) => Ok(Some(data::from_parquet(bytes.clone())?)),
            None => Ok(None),
        }
    }

    /// Premier nœud en erreur (nom, message).
    pub fn first_error(&self) -> Option<(&str, &str)> {
        self.order.iter().find_map(|id| {
            let r = self.results.get(id)?;
            (r.status == Status::Error).then(|| (id.as_str(), r.error.as_deref().unwrap_or("")))
        })
    }
}

/// Données ayant déclenché l'exécution (webhook reçu, formulaire soumis, nouveaux éléments…).
#[derive(Debug, Clone)]
pub struct TriggerEvent {
    /// Identifiant du nœud déclencheur.
    pub node_id: String,
    pub data: RecordBatch,
}

#[derive(Debug, Clone, Default)]
pub struct RunOptions<'a> {
    /// N'exécuter que ce nœud et ses ancêtres.
    pub target: Option<&'a str>,
    /// Exécution automatique : seul ce déclencheur produit des données.
    pub trigger: Option<TriggerEvent>,
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

/// Exécution manuelle, sans hôte. Avec `target`, seuls ce nœud et ses ancêtres sont exécutés.
pub async fn run(wf: &Workflow, target: Option<&str>) -> RunReport {
    run_with(
        wf,
        RunOptions {
            target,
            trigger: None,
        },
        &NoHost,
    )
    .await
}

/// Exécute le workflow avec un hôte (réseau, sous-workflows) et éventuellement un déclencheur.
pub async fn run_with(wf: &Workflow, opts: RunOptions<'_>, host: &dyn Host) -> RunReport {
    let start = now_ms();
    let order = match wf.topological_order() {
        Ok(o) => o,
        Err(e) => return RunReport::failure(e.to_string()),
    };
    if let Some(t) = &opts.trigger {
        match wf.nodes.iter().find(|n| n.id == t.node_id) {
            Some(n) if triggers::is_trigger(&n.kind) => {}
            _ => return RunReport::failure(format!("Déclencheur introuvable : {}", t.node_id)),
        }
    }
    let order: Vec<String> = match opts.target {
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
        let result = match run_node(wf, node, &results, opts.trigger.as_ref(), host).await {
            Ok(Outcome::Data(batch)) => match data::to_parquet(&batch) {
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
            Ok(Outcome::Skipped(reason)) => NodeResult::failed(Status::Skipped, reason, 0.0),
            Err(e) => NodeResult::failed(Status::Error, e.to_string(), now_ms() - t0),
        };
        results.insert(id.clone(), result);
    }

    // Un déclencheur non concerné (et sa branche) est ignoré sans être une erreur.
    let ok = results.values().all(|r| r.status != Status::Error);
    let output = order
        .iter()
        .rev()
        .find(|id| {
            results
                .get(*id)
                .is_some_and(|r| r.status == Status::Success)
                && !wf.edges.iter().any(|e| &e.source == *id)
        })
        .cloned();
    RunReport {
        ok,
        error: None,
        order,
        results,
        duration_ms: now_ms() - start,
        output,
    }
}

enum Outcome {
    Data(RecordBatch),
    Skipped(String),
}

const UPSTREAM_SKIPPED: &str = "Une entrée en amont n'a pas produit de données";

async fn run_node(
    wf: &Workflow,
    node: &Node,
    results: &BTreeMap<String, NodeResult>,
    trigger: Option<&TriggerEvent>,
    host: &dyn Host,
) -> Result<Outcome> {
    if triggers::is_trigger(&node.kind) {
        return match trigger {
            Some(t) if t.node_id == node.id => Ok(Outcome::Data(t.data.clone())),
            Some(_) => Ok(Outcome::Skipped(
                "Déclencheur non concerné par cette exécution".into(),
            )),
            None if node.disabled => Ok(Outcome::Skipped("Déclencheur désactivé".into())),
            None => triggers::test_output(node, host).await.map(Outcome::Data),
        };
    }
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
                None => return Ok(Outcome::Skipped(UPSTREAM_SKIPPED.into())),
            }
        }
        inputs.push(data::append_batches(&batches)?);
    }
    if node.disabled {
        return Ok(match inputs.into_iter().next() {
            Some(b) => Outcome::Data(b),
            None => Outcome::Skipped("Nœud désactivé".into()),
        });
    }
    // `$('Nom')` : sortie d'un nœud déjà exécuté, décodée à la demande.
    let lookup = |name: &str| -> Option<RecordBatch> {
        let other = wf
            .nodes
            .iter()
            .find(|n| n.name == name && results.contains_key(&n.id))?;
        let bytes = results.get(&other.id)?.parquet.as_ref()?;
        data::from_parquet(bytes.clone()).ok()
    };
    let ctx = nodes::NodeCtx {
        lookup: &lookup,
        node_names: wf
            .nodes
            .iter()
            .filter(|n| results.contains_key(&n.id))
            .map(|n| n.name.clone())
            .collect(),
    };
    nodes::execute(node, inputs, host, &ctx)
        .await
        .map(Outcome::Data)
}
