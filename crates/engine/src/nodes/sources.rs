use arrow::array::RecordBatch;

use super::Params;
use crate::data::{csv_to_batch, json_to_batch};
use crate::error::{Result, fail};

/// Taille maximale d'un contenu saisi dans un nœud.
const MAX_INLINE_BYTES: usize = 20 * 1024 * 1024;

fn check_size(text: &str, what: &str) -> Result<()> {
    if text.len() > MAX_INLINE_BYTES {
        return fail(format!("{what} trop volumineux (20 Mo max.)"));
    }
    Ok(())
}

pub fn manual(p: &Params) -> Result<RecordBatch> {
    let text = p.str("data");
    check_size(&text, "JSON")?;
    if text.trim().is_empty() {
        return json_to_batch(&serde_json::Value::Array(vec![]));
    }
    let doc: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| crate::error::EngineError::Msg(format!("JSON invalide : {e}")))?;
    json_to_batch(&doc)
}

pub fn csv(p: &Params) -> Result<RecordBatch> {
    let delim = p.str_or("delimiter", ",");
    let delim = match delim.as_str() {
        "\\t" | "tab" => b'\t',
        d if d.len() == 1 => d.as_bytes()[0],
        d => {
            return fail(format!(
                "Séparateur invalide « {d} » (un seul caractère attendu)"
            ));
        }
    };
    let text = p.str("csv");
    check_size(&text, "CSV")?;
    csv_to_batch(&text, delim, p.bool("header", true))
}
