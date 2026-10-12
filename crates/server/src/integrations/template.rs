//! Modèles de texte : `{{colonne}}` est remplacé par la valeur de la ligne.
//! En mode récapitulatif : `{{nb_lignes}}` et `{{tableau}}` (20 premières lignes).

use serde_json::{Map, Value};

fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Remplace les `{{nom}}` par les valeurs de `vars` (inconnus : laissés vides).
pub fn render(template: &str, vars: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = after[..end].trim();
                if let Some(v) = vars.get(key) {
                    out.push_str(&text(v));
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Tableau texte des premières lignes (messages récapitulatifs).
pub fn table(rows: &[Map<String, Value>], max: usize) -> String {
    let Some(first) = rows.first() else {
        return String::new();
    };
    let cols: Vec<&String> = first.keys().take(8).collect();
    let mut out = cols
        .iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    for row in rows.iter().take(max) {
        out.push('\n');
        let cells: Vec<String> = cols
            .iter()
            .map(|c| {
                let t = row.get(*c).map(text).unwrap_or_default();
                t.chars().take(40).collect()
            })
            .collect();
        out.push_str(&cells.join(" | "));
    }
    if rows.len() > max {
        out.push_str(&format!("\n… et {} autre(s) ligne(s)", rows.len() - max));
    }
    out
}

/// Variables du mode récapitulatif.
pub fn summary_vars(rows: &[Map<String, Value>]) -> Map<String, Value> {
    let mut vars = rows.first().cloned().unwrap_or_default();
    vars.insert("nb_lignes".into(), Value::from(rows.len()));
    vars.insert("tableau".into(), Value::String(table(rows, 20)));
    vars
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders() {
        let row = json!({"nom": "Ada", "n": 3, "vide": null})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(
            render("Bonjour {{ nom }} ({{n}}){{vide}}{{inconnu}} !", &row),
            "Bonjour Ada (3) !"
        );
        assert_eq!(render("{{ouvert", &row), "{{ouvert");
        let rows = vec![row.clone(), row];
        let vars = summary_vars(&rows);
        assert_eq!(render("{{nb_lignes}} lignes", &vars), "2 lignes");
        assert!(render("{{tableau}}", &vars).starts_with("nom | n | vide\nAda | 3 | "));
    }
}
