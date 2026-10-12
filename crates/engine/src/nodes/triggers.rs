//! Déclencheurs : nœuds de départ d'un workflow.
//!
//! - Exécution automatique (serveur) : seul le déclencheur qui a démarré l'exécution
//!   produit des données (celles reçues : requête webhook, formulaire, nouveaux éléments…),
//!   les autres sont ignorés avec leur branche.
//! - Exécution manuelle (éditeur) : chaque déclencheur produit ses données de test.

use arrow::array::RecordBatch;
use serde::Serialize;
use serde_json::{Map, Value as Json, json};

use super::{Params, http};
use crate::data::json_to_batch;
use crate::error::{EngineError, Result, fail};
use crate::host::{Host, now_iso};
use crate::workflow::Node;

pub const MANUAL: &str = "trigger_manual";
pub const SCHEDULE: &str = "trigger_schedule";
pub const WEBHOOK: &str = "trigger_webhook";
pub const FORM: &str = "trigger_form";
pub const POLL: &str = "trigger_poll";
pub const WORKFLOW: &str = "trigger_workflow";
pub const ERROR: &str = "trigger_error";

pub fn is_trigger(kind: &str) -> bool {
    matches!(
        kind,
        MANUAL | SCHEDULE | WEBHOOK | FORM | POLL | WORKFLOW | ERROR
    )
}

/// Données de test du déclencheur (exécution manuelle depuis l'éditeur).
pub async fn test_output(node: &Node, host: &dyn Host) -> Result<RecordBatch> {
    let p = Params(&node.params);
    let now = now_iso();
    match node.kind.as_str() {
        MANUAL => json_to_batch(&json!({ "declenche_le": now, "mode": "manuel" })),
        SCHEDULE => json_to_batch(&json!({ "declenche_le": now, "mode": "planification" })),
        WEBHOOK | WORKFLOW => test_data(&p, json!({ "exemple": "valeur", "nombre": 42 })),
        FORM => {
            let mut sample = Map::new();
            for f in parse_form_fields(&p.str("fields"))? {
                let v = match f.kind.as_str() {
                    "nombre" => json!(0),
                    "case" => json!(false),
                    _ => json!(""),
                };
                sample.insert(f.key, v);
            }
            sample.insert("soumis_le".into(), json!(now));
            test_data(&p, Json::Object(sample))
        }
        POLL => http::request(&p, host, &crate::expr::Scope::new(None)).await,
        ERROR => json_to_batch(&json!({
            "workflow": "Workflow en échec",
            "workflow_id": "00000000-0000-0000-0000-000000000000",
            "execution_id": "00000000-0000-0000-0000-000000000000",
            "noeud": "Filtrer",
            "erreur": "Colonne inconnue « prix »",
            "date": now,
        })),
        other => fail(format!("Déclencheur inconnu : {other}")),
    }
}

/// Paramètre `test_data` (JSON) s'il est renseigné, sinon l'exemple fourni.
fn test_data(p: &Params, fallback: Json) -> Result<RecordBatch> {
    let text = p.str("test_data");
    if text.trim().is_empty() {
        return json_to_batch(&fallback);
    }
    let doc: Json = serde_json::from_str(&text)
        .map_err(|e| EngineError::Msg(format!("Données de test : JSON invalide ({e})")))?;
    json_to_batch(&doc)
}

/// Champ d'un formulaire public.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FormField {
    /// Nom de colonne (dérivé du libellé).
    pub key: String,
    pub label: String,
    /// texte | texte long | email | nombre | date | case
    pub kind: String,
    pub required: bool,
}

pub const FIELD_KINDS: &[&str] = &["texte", "texte long", "email", "nombre", "date", "case"];

/// Une ligne par champ : `Libellé | type | requis` (type et « requis » facultatifs).
pub fn parse_form_fields(text: &str) -> Result<Vec<FormField>> {
    let mut fields: Vec<FormField> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut parts = line.split('|').map(str::trim);
        let label = parts.next().unwrap_or_default().to_string();
        if label.is_empty() || label.chars().count() > 80 {
            return fail(format!("Champ invalide : « {line} »"));
        }
        let kind = parts
            .next()
            .filter(|k| !k.is_empty())
            .unwrap_or("texte")
            .to_lowercase();
        if !FIELD_KINDS.contains(&kind.as_str()) {
            return fail(format!(
                "Type de champ inconnu « {kind} » (types : {})",
                FIELD_KINDS.join(", ")
            ));
        }
        let required = parts
            .next()
            .is_some_and(|r| r.eq_ignore_ascii_case("requis"));
        let key = field_key(&label);
        if key.is_empty() || fields.iter().any(|f| f.key == key) {
            return fail(format!("Champ en double ou sans nom : « {label} »"));
        }
        fields.push(FormField {
            key,
            label,
            kind,
            required,
        });
    }
    if fields.len() > 50 {
        return fail("50 champs au maximum");
    }
    Ok(fields)
}

/// « Adresse e-mail » → « adresse_e_mail ».
fn field_key(label: &str) -> String {
    let mut key = String::new();
    for c in label.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            key.push(c);
        } else if !key.ends_with('_') && !key.is_empty() {
            key.push('_');
        }
    }
    key.trim_end_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_fields() {
        let f = parse_form_fields(
            "Nom | texte | requis\nAdresse e-mail | email | requis\nÂge | nombre\nMessage",
        )
        .unwrap();
        assert_eq!(f.len(), 4);
        assert_eq!(f[1].key, "adresse_e_mail");
        assert!(f[0].required && !f[2].required);
        assert_eq!(f[3].kind, "texte");
        assert!(parse_form_fields("Nom\nnom").is_err());
        assert!(parse_form_fields("X | inconnu").is_err());
    }
}
