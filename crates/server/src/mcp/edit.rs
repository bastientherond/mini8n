//! Modifications d'une définition de workflow (format de l'éditeur) demandées par une IA.
//! Mêmes règles que l'interface : types et paramètres connus, pas de boucle, pas de lien
//! en double, un nœud intermédiaire supprimé garde la liaison.

use mini8n_engine::catalog::{self, NodeSpec};
use serde_json::{Map, Value, json};

use crate::workflows::valid_id;

/// Placement automatique, aligné sur la mise en page de l'éditeur (nœud de 240 × 92,
/// espacement 120 × 60).
const GAP_X: f64 = 360.0;
const GAP_Y: f64 = 160.0;
const NODE_W: f64 = 240.0;
const NODE_H: f64 = 92.0;

pub type EditResult<T> = Result<T, String>;

fn nodes(def: &Value) -> &[Value] {
    def["nodes"].as_array().map_or(&[], Vec::as_slice)
}

fn edges(def: &Value) -> &[Value] {
    def["edges"].as_array().map_or(&[], Vec::as_slice)
}

fn nodes_mut(def: &mut Value) -> &mut Vec<Value> {
    if !def["nodes"].is_array() {
        def["nodes"] = json!([]);
    }
    def["nodes"].as_array_mut().expect("tableau")
}

fn edges_mut(def: &mut Value) -> &mut Vec<Value> {
    if !def["edges"].is_array() {
        def["edges"] = json!([]);
    }
    def["edges"].as_array_mut().expect("tableau")
}

pub fn find<'a>(def: &'a Value, id: &str) -> EditResult<&'a Value> {
    nodes(def)
        .iter()
        .find(|n| n["id"] == id)
        .ok_or_else(|| format!("Nœud introuvable : {id}"))
}

fn find_mut<'a>(def: &'a mut Value, id: &str) -> EditResult<&'a mut Value> {
    nodes_mut(def)
        .iter_mut()
        .find(|n| n["id"] == id)
        .ok_or_else(|| format!("Nœud introuvable : {id}"))
}

pub fn spec_of(node: &Value) -> EditResult<NodeSpec> {
    let kind = node
        .pointer("/data/kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    catalog::spec(kind).ok_or_else(|| format!("Type de nœud inconnu : {kind}"))
}

fn handle_index(edge: &Value) -> usize {
    edge["targetHandle"]
        .as_str()
        .and_then(|h| h.strip_prefix("in-"))
        .and_then(|i| i.parse().ok())
        .unwrap_or(0)
}

fn random_suffix() -> String {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).expect("aléa du système");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Paramètres fusionnés : `null` rétablit la valeur par défaut ; un nom inconnu est refusé.
fn merge_params(
    spec: &NodeSpec,
    current: &mut Map<String, Value>,
    changes: &Map<String, Value>,
) -> EditResult<()> {
    for (name, value) in changes {
        let Some(p) = spec.params.iter().find(|p| p.name == name) else {
            let known: Vec<_> = spec.params.iter().map(|p| p.name).collect();
            return Err(format!(
                "Paramètre inconnu pour « {} » : {name} (paramètres : {})",
                spec.kind,
                known.join(", ")
            ));
        };
        if !p.options.is_empty()
            && !value.is_null()
            && !value.as_str().is_some_and(|v| p.options.contains(&v))
        {
            return Err(format!(
                "{name} : valeur attendue parmi {}",
                p.options.join(", ")
            ));
        }
        if matches!(value, Value::Array(_) | Value::Object(_)) {
            return Err(format!(
                "{name} : valeur simple attendue (texte, nombre ou booléen)"
            ));
        }
        current.insert(
            name.clone(),
            if value.is_null() {
                p.default.clone()
            } else {
                value.clone()
            },
        );
    }
    Ok(())
}

/// Nom libre dérivé de `base` (« Filtrer », « Filtrer 2 »…) : `$('Nom')` reste sans ambiguïté.
fn unique_name(def: &Value, base: &str) -> String {
    let taken = |n: &str| {
        nodes(def)
            .iter()
            .any(|x| x.pointer("/data/name").and_then(Value::as_str) == Some(n))
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .expect("nom libre")
}

/// Modifie les paramètres, le nom ou l'état d'un nœud.
pub fn update_node(
    def: &mut Value,
    id: &str,
    params: Option<&Map<String, Value>>,
    name: Option<&str>,
    disabled: Option<bool>,
) -> EditResult<()> {
    let spec = spec_of(find(def, id)?)?;
    if let Some(name) = name {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return Err("Nom invalide (1 à 120 caractères)".into());
        }
        if nodes(def)
            .iter()
            .any(|n| n["id"] != id && n.pointer("/data/name").and_then(Value::as_str) == Some(name))
        {
            return Err(format!("Un autre nœud s'appelle déjà « {name} »"));
        }
    }
    let node = find_mut(def, id)?;
    let data = node["data"].as_object_mut().ok_or("nœud mal formé")?;
    if let Some(changes) = params {
        let mut current = data
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        merge_params(&spec, &mut current, changes)?;
        data.insert("params".into(), Value::Object(current));
    }
    if let Some(name) = name {
        data.insert("name".into(), json!(name.trim()));
    }
    if let Some(d) = disabled {
        data.insert("disabled".into(), json!(d));
    }
    Ok(())
}

/// Ajoute un nœud, éventuellement relié après `from` et/ou avant `to` (entrée `input`).
pub fn add_node(
    def: &mut Value,
    kind: &str,
    name: Option<&str>,
    params: Option<&Map<String, Value>>,
    from: Option<&str>,
    to: Option<(&str, usize)>,
) -> EditResult<String> {
    let spec = catalog::spec(kind)
        .ok_or_else(|| format!("Type de nœud inconnu : {kind} (voir list_node_types)"))?;
    let pos = |n: &Value, axis: &str| {
        n.pointer(&format!("/position/{axis}"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    let wanted = match (from, to) {
        (Some(f), _) => {
            let n = find(def, f)?;
            json!({ "x": pos(n, "x") + GAP_X, "y": pos(n, "y") })
        }
        (None, Some((t, _))) => {
            let n = find(def, t)?;
            json!({ "x": pos(n, "x") - GAP_X, "y": pos(n, "y") })
        }
        _ => {
            let max_x = nodes(def)
                .iter()
                .map(|n| pos(n, "x"))
                .fold(f64::NEG_INFINITY, f64::max);
            let y = nodes(def).iter().map(|n| pos(n, "y")).fold(0.0, f64::max);
            json!({ "x": if max_x.is_finite() { max_x + GAP_X } else { 0.0 }, "y": if max_x.is_finite() { y } else { 0.0 } })
        }
    };
    // Place occupée (branches parallèles) : le nœud descend jusqu'à une place libre.
    let x = wanted["x"].as_f64().unwrap_or(0.0);
    let mut y = wanted["y"].as_f64().unwrap_or(0.0);
    while nodes(def)
        .iter()
        .any(|n| (pos(n, "x") - x).abs() < NODE_W + 40.0 && (pos(n, "y") - y).abs() < NODE_H + 40.0)
    {
        y += GAP_Y;
    }
    let position = json!({ "x": x, "y": y });
    let mut values = Map::new();
    for p in &spec.params {
        values.insert(p.name.into(), p.default.clone());
    }
    if let Some(changes) = params {
        merge_params(&spec, &mut values, changes)?;
    }
    let id = format!("{kind}-{}", random_suffix());
    let name = unique_name(
        def,
        name.map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(spec.label),
    );
    nodes_mut(def).push(json!({
        "id": id,
        "position": position,
        "data": { "kind": kind, "name": name, "params": values, "disabled": false },
    }));
    // Insertion sur un lien existant from → to : il est remplacé par from → nouveau → to.
    if let (Some(f), Some((t, input))) = (from, to) {
        let handle = format!("in-{input}");
        edges_mut(def).retain(|e| {
            !(e["source"] == f && e["target"] == t && e["targetHandle"] == handle.as_str())
        });
    }
    if let Some(f) = from {
        connect(def, f, &id, 0)?;
    }
    if let Some((t, input)) = to {
        connect(def, &id, t, input)?;
    }
    Ok(id)
}

/// Supprime un nœud ; un nœud intermédiaire garde la liaison (prédécesseurs → successeurs).
pub fn delete_node(def: &mut Value, id: &str) -> EditResult<()> {
    find(def, id)?;
    let before: Vec<String> = edges(def)
        .iter()
        .filter(|e| e["target"] == id && handle_index(e) == 0)
        .filter_map(|e| e["source"].as_str().map(String::from))
        .collect();
    let after: Vec<(String, usize)> = edges(def)
        .iter()
        .filter(|e| e["source"] == id)
        .filter_map(|e| Some((e["target"].as_str()?.to_string(), handle_index(e))))
        .collect();
    nodes_mut(def).retain(|n| n["id"] != id);
    edges_mut(def).retain(|e| e["source"] != id && e["target"] != id);
    for b in &before {
        for (t, input) in &after {
            // Une liaison impossible (doublon) est simplement ignorée.
            let _ = connect(def, b, t, *input);
        }
    }
    Ok(())
}

/// Le lien source → cible créerait-il une boucle ?
fn creates_cycle(def: &Value, source: &str, target: &str) -> bool {
    let mut stack = vec![target.to_string()];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = stack.pop() {
        if id == source {
            return true;
        }
        if seen.insert(id.clone()) {
            stack.extend(
                edges(def)
                    .iter()
                    .filter(|e| e["source"] == id.as_str())
                    .filter_map(|e| e["target"].as_str().map(String::from)),
            );
        }
    }
    false
}

/// Relie la sortie de `source` à l'entrée `input` de `target`.
pub fn connect(def: &mut Value, source: &str, target: &str, input: usize) -> EditResult<()> {
    if source == target {
        return Err("Un nœud ne peut pas être relié à lui-même".into());
    }
    find(def, source)?;
    let spec = spec_of(find(def, target)?)?;
    if spec.inputs.is_empty() {
        return Err(format!("« {} » n'a pas d'entrée (déclencheur)", spec.label));
    }
    if input >= spec.inputs.len() {
        return Err(format!(
            "« {} » a {} entrée(s) : input de 0 à {}",
            spec.label,
            spec.inputs.len(),
            spec.inputs.len() - 1
        ));
    }
    let handle = format!("in-{input}");
    if edges(def).iter().any(|e| {
        e["source"] == source && e["target"] == target && e["targetHandle"] == handle.as_str()
    }) {
        return Err("Ces nœuds sont déjà reliés".into());
    }
    if creates_cycle(def, source, target) {
        return Err("Ce lien créerait une boucle".into());
    }
    let id = format!("e-{}", random_suffix());
    debug_assert!(valid_id(&id));
    edges_mut(def).push(json!({ "id": id, "source": source, "target": target, "sourceHandle": "out", "targetHandle": handle }));
    Ok(())
}

/// Retire le(s) lien(s) source → cible (toutes les entrées si `input` est absent).
pub fn disconnect(
    def: &mut Value,
    source: &str,
    target: &str,
    input: Option<usize>,
) -> EditResult<()> {
    let before = edges(def).len();
    edges_mut(def).retain(|e| {
        !(e["source"] == source
            && e["target"] == target
            && input.is_none_or(|i| handle_index(e) == i))
    });
    if edges(def).len() == before {
        return Err("Aucun lien entre ces nœuds".into());
    }
    Ok(())
}

/// Vue compacte d'un workflow pour une IA : nœuds (paramètres, entrées reliées) et liens.
pub fn describe(def: &Value) -> Value {
    let list: Vec<Value> = nodes(def)
        .iter()
        .map(|n| {
            let id = n["id"].as_str().unwrap_or_default();
            let spec = spec_of(n).ok();
            let inputs: Vec<Value> = spec
                .as_ref()
                .map(|s| {
                    s.inputs
                        .iter()
                        .enumerate()
                        .map(|(i, label)| {
                            let from: Vec<&Value> = edges(def)
                                .iter()
                                .filter(|e| e["target"] == id && handle_index(e) == i)
                                .map(|e| &e["source"])
                                .collect();
                            json!({ "input": i, "label": label, "from": from, "optional": s.optional_input })
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "id": id,
                "name": n.pointer("/data/name"),
                "kind": n.pointer("/data/kind"),
                "type": spec.as_ref().map(|s| s.label),
                "disabled": n.pointer("/data/disabled").and_then(Value::as_bool).unwrap_or(false),
                "params": n.pointer("/data/params"),
                "inputs": inputs,
                "outputs_to": edges(def).iter().filter(|e| e["source"] == id).map(|e| json!({ "node": e["target"], "input": handle_index(e) })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "nodes": list })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wf() -> Value {
        json!({
            "nodes": [
                {"id": "t", "position": {"x": 0, "y": 0}, "data": {"kind": "trigger_manual", "name": "Départ", "params": {}}},
                {"id": "f", "position": {"x": 260, "y": 0}, "data": {"kind": "filter", "name": "Filtrer", "params": {"condition": "true"}}},
                {"id": "s", "position": {"x": 520, "y": 0}, "data": {"kind": "sort", "name": "Trier", "params": {"by": ""}}}
            ],
            "edges": [
                {"id": "e1", "source": "t", "target": "f", "targetHandle": "in-0"},
                {"id": "e2", "source": "f", "target": "s", "targetHandle": "in-0"}
            ]
        })
    }

    #[test]
    fn edits() {
        let mut d = wf();
        update_node(
            &mut d,
            "f",
            Some(json!({"condition": "$json.prix > 10"}).as_object().unwrap()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            d["nodes"][1]["data"]["params"]["condition"],
            "$json.prix > 10"
        );
        let err = update_node(
            &mut d,
            "f",
            Some(json!({"nope": 1}).as_object().unwrap()),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.contains("Paramètre inconnu"), "{err}");
        assert!(
            update_node(&mut d, "f", None, Some("Trier"), None).is_err(),
            "nom déjà pris"
        );

        // Insertion sur le lien f → s.
        let id = add_node(&mut d, "limit", None, None, Some("f"), Some(("s", 0))).unwrap();
        assert_eq!(find(&d, &id).unwrap()["data"]["params"]["count"], 10);
        assert!(
            !edges(&d)
                .iter()
                .any(|e| e["source"] == "f" && e["target"] == "s")
        );
        assert!(connect(&mut d, "s", "f", 0).unwrap_err().contains("boucle"));
        assert!(
            connect(&mut d, "f", "t", 0)
                .unwrap_err()
                .contains("pas d'entrée")
        );

        // Deux nœuds après le même prédécesseur ne se superposent pas.
        let a = add_node(&mut d, "limit", None, None, Some("t"), None).unwrap();
        let (fy, ay) = (
            &find(&d, "f").unwrap()["position"]["y"],
            &find(&d, &a).unwrap()["position"]["y"],
        );
        assert!(ay.as_f64().unwrap() > fy.as_f64().unwrap(), "{fy} / {ay}");
        delete_node(&mut d, &a).unwrap();

        // Suppression : la liaison f → s est rétablie.
        delete_node(&mut d, &id).unwrap();
        assert!(
            edges(&d)
                .iter()
                .any(|e| e["source"] == "f" && e["target"] == "s")
        );
        disconnect(&mut d, "f", "s", None).unwrap();
        assert!(disconnect(&mut d, "f", "s", None).is_err());

        let view = describe(&d);
        assert_eq!(view["nodes"][1]["inputs"][0]["from"], json!(["t"]));
        assert_eq!(
            add_node(&mut d, "filter", None, None, None, None)
                .map(|id| find(&d, &id).unwrap()["data"]["name"].clone())
                .unwrap(),
            "Filtrer 2"
        );
    }
}
