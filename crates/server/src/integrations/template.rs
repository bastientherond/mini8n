//! Textes des intégrations avec expressions `{{ … }}` (langage du moteur).
//! En mode récapitulatif, `nb_lignes` et `tableau` (20 premières lignes) sont
//! disponibles, et `$json` désigne la première ligne.

use mini8n_engine::arrow::array::RecordBatch;
use mini8n_engine::expr::{Scope, Template};
use mini8n_engine::value::{RowAccessor, Value as Scalar};
use serde_json::{Map, Value};

use super::rows_of;

fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
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

fn compile(fields: &[(&str, &str)], scope: &Scope) -> Result<Vec<Template>, String> {
    fields
        .iter()
        .map(|(name, src)| Template::compile(src, scope).map_err(|e| format!("{name} : {e}")))
        .collect()
}

/// Textes rendus pour chaque ligne de l'entrée (`fields` : nom affiché, gabarit).
pub fn per_row(fields: &[(&str, &str)], input: &RecordBatch) -> Result<Vec<Vec<String>>, String> {
    let templates = compile(fields, &Scope::new(Some(input)))?;
    let rows = RowAccessor::new(input).map_err(|e| e.to_string())?;
    (0..input.num_rows())
        .map(|r| {
            templates
                .iter()
                .zip(fields)
                .map(|(t, (name, _))| {
                    t.render(Some(&rows), r)
                        .map_err(|e| format!("{name}, ligne {} : {e}", r + 1))
                })
                .collect()
        })
        .collect()
}

/// Lignes répétées au plus dans un récapitulatif.
const MAX_LINES: usize = 1000;

/// Textes rendus une seule fois (mode récapitulatif, nom de fichier…).
pub fn summary(fields: &[(&str, &str)], input: &RecordBatch) -> Result<Vec<String>, String> {
    summary_message(fields, input, "")
}

/// Récapitulatif d'un message : `lignes` contient le gabarit `row` rendu pour chaque ligne
/// de l'entrée (le tableau si `row` est vide). Si aucun champ n'utilise `lignes`, elles
/// sont ajoutées à la fin du dernier champ (le corps du message).
pub fn summary_message(
    fields: &[(&str, &str)],
    input: &RecordBatch,
    row: &str,
) -> Result<Vec<String>, String> {
    let json_rows = rows_of(input)?;
    let tableau = table(&json_rows, 20);
    let lines = if row.trim().is_empty() {
        None
    } else {
        let mut lines: Vec<String> = per_row(
            &[("Ligne répétée", row)],
            &input.slice(0, input.num_rows().min(MAX_LINES)),
        )?
        .into_iter()
        .flatten()
        .collect();
        if input.num_rows() > MAX_LINES {
            lines.push(format!(
                "… et {} autre(s) ligne(s)",
                input.num_rows() - MAX_LINES
            ));
        }
        Some(lines.join("\n"))
    };
    let uses_lines = fields
        .iter()
        .any(|(_, src)| src.replace("nb_lignes", "").contains("lignes"));
    let scope = Scope::new(Some(input))
        .with_var("nb_lignes", Scalar::Int(input.num_rows() as i64))
        .with_var(
            "lignes",
            Scalar::Str(lines.clone().unwrap_or_else(|| tableau.clone())),
        )
        .with_var("tableau", Scalar::Str(tableau));
    let templates = compile(fields, &scope)?;
    let rows = RowAccessor::new(input).map_err(|e| e.to_string())?;
    let rows = (input.num_rows() > 0).then_some(&rows);
    templates
        .iter()
        .zip(fields)
        .map(|(t, (name, _))| t.render(rows, 0).map_err(|e| format!("{name} : {e}")))
        .collect::<Result<Vec<_>, _>>()
        .map(|mut out| {
            if let (Some(lines), false, Some(last)) = (lines, uses_lines, out.last_mut()) {
                if !last.is_empty() && !last.ends_with('\n') {
                    last.push('\n');
                }
                last.push_str(&lines);
            }
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mini8n_engine::data::json_to_batch;
    use serde_json::json;

    #[test]
    fn renders() {
        let input = json_to_batch(&json!([
            {"nom": "Ada", "n": 3, "vide": null},
            {"nom": "Alan", "n": 4, "vide": null}
        ]))
        .unwrap();
        let rows = per_row(
            &[(
                "Message",
                "Bonjour {{ $json.nom }} ({{ $json.n * 2 }}){{ $json.vide }} !",
            )],
            &input,
        )
        .unwrap();
        assert_eq!(rows[0][0], "Bonjour Ada (6) !");
        assert_eq!(rows[1][0], "Bonjour Alan (8) !");
        // Ancienne syntaxe : noms de colonnes nus.
        assert_eq!(per_row(&[("x", "{{nom}}")], &input).unwrap()[1][0], "Alan");

        let out = summary(
            &[
                ("Objet", "{{nb_lignes}} lignes"),
                ("Corps", "{{ tableau }}"),
                ("Total", "{{ $input.all().map(i => i.json.n).sum() }}"),
            ],
            &input,
        )
        .unwrap();
        assert_eq!(out[0], "2 lignes");
        assert!(out[1].starts_with("nom | n | vide\nAda | 3 | "));
        assert_eq!(out[2], "7");

        // Ligne répétée : à la place de {{ lignes }}, sinon à la fin du message.
        let row = "- {{ $json.nom }} : {{ $json.n }}";
        let out = summary_message(
            &[
                ("Objet", "{{ nb_lignes }} lignes"),
                ("Corps", "Liste :\n{{ lignes }}\nFin"),
            ],
            &input,
            row,
        )
        .unwrap();
        assert_eq!(out, ["2 lignes", "Liste :\n- Ada : 3\n- Alan : 4\nFin"]);
        let out = summary_message(&[("Corps", "Bonjour,")], &input, row).unwrap();
        assert_eq!(out[0], "Bonjour,\n- Ada : 3\n- Alan : 4");

        let e = per_row(&[("Objet", "{{ $json.nmo }}")], &input).unwrap_err();
        assert!(e.starts_with("Objet : Colonne inconnue"), "{e}");
    }
}
