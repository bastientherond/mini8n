//! Nœuds « Créer un fichier » et « Lire un fichier » (fichiers du workspace).

use std::io::Cursor;

use calamine::{Data, Reader, open_workbook_auto_from_rs};
use chrono::Utc;
use mini8n_engine::arrow::array::RecordBatch;
use mini8n_engine::data;
use mini8n_engine::value::{RowAccessor, Value as Cell};
use rust_xlsxwriter::{Format, Workbook};
use serde_json::{Map, Value, json};

use super::{Ctx, str_param, table_of};
use crate::files;

const EXCEL_MAX_ROWS: usize = 1_048_575;

/// Contenu Excel (.xlsx) d'une table : en-têtes en gras, valeurs typées.
pub fn to_xlsx(batch: &RecordBatch) -> Result<Vec<u8>, String> {
    if batch.num_rows() > EXCEL_MAX_ROWS {
        return Err("Trop de lignes pour Excel (1 048 575 max.)".into());
    }
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet();
    let bold = Format::new().set_bold();
    let schema = batch.schema();
    let err = |e: rust_xlsxwriter::XlsxError| e.to_string();
    for (c, f) in schema.fields().iter().enumerate() {
        sheet
            .write_string_with_format(0, c as u16, f.name(), &bold)
            .map_err(err)?;
    }
    let rows = RowAccessor::new(batch).map_err(|e| e.to_string())?;
    for r in 0..batch.num_rows() {
        for c in 0..batch.num_columns() {
            let (row, col) = (r as u32 + 1, c as u16);
            match rows.get(c, r) {
                Cell::Null => {}
                Cell::Bool(b) => {
                    sheet.write_boolean(row, col, b).map_err(err)?;
                }
                Cell::Int(i) => {
                    sheet.write_number(row, col, i as f64).map_err(err)?;
                }
                Cell::Float(f) => {
                    sheet.write_number(row, col, f).map_err(err)?;
                }
                Cell::Str(s) => {
                    sheet
                        .write_string(row, col, s.chars().take(32_767).collect::<String>())
                        .map_err(err)?;
                }
            }
        }
    }
    sheet.autofit();
    wb.save_to_buffer().map_err(err)
}

fn render_name(template: &str, rows: usize, ext: &str) -> String {
    let now = Utc::now();
    let base = if template.trim().is_empty() {
        "export-{date}"
    } else {
        template.trim()
    };
    let name = base
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{heure}", &now.format("%H-%M-%S").to_string())
        .replace("{lignes}", &rows.to_string());
    if name.to_lowercase().ends_with(&format!(".{ext}")) {
        name
    } else {
        format!("{name}.{ext}")
    }
}

pub async fn write(
    ctx: &Ctx,
    p: &Map<String, Value>,
    input: RecordBatch,
) -> Result<RecordBatch, String> {
    let format = str_param(p, "format");
    let (ext, bytes) = match format.as_str() {
        "json" => {
            let json = data::batch_to_json(&input).map_err(|e| e.to_string())?;
            (
                "json",
                serde_json::to_vec_pretty(&json).map_err(|e| e.to_string())?,
            )
        }
        "parquet" => (
            "parquet",
            data::to_parquet(&input).map_err(|e| e.to_string())?,
        ),
        "excel" => ("xlsx", to_xlsx(&input)?),
        _ => (
            "csv",
            data::batch_to_csv(&input)
                .map_err(|e| e.to_string())?
                .into_bytes(),
        ),
    };
    let name = render_name(&str_param(p, "name"), input.num_rows(), ext);
    let file = files::store(
        &ctx.db,
        ctx.workspace_id,
        &name,
        &bytes,
        ctx.user_id,
        ctx.execution_id,
    )
    .await?;
    table_of(json!({
        "fichier": file.name,
        "id": file.id.to_string(),
        "format": ext,
        "taille": file.size,
        "lignes": input.num_rows(),
        "cree_le": file.created_at.to_rfc3339(),
    }))
}

/// Séparateur le plus probable d'après la première ligne.
fn sniff_delimiter(text: &str) -> u8 {
    let line = text.lines().next().unwrap_or_default();
    b";\t|,"
        .iter()
        .copied()
        .max_by_key(|d| (line.bytes().filter(|b| b == d).count(), *d == b','))
        .unwrap_or(b',')
}

fn excel_cell(cell: &Data) -> Value {
    match cell {
        Data::Empty | Data::Error(_) => Value::Null,
        Data::Int(i) => json!(i),
        Data::Float(f) => json!(f),
        Data::Bool(b) => json!(b),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => json!(s),
        Data::DateTime(d) => d
            .as_datetime()
            .map(|dt| {
                if dt.time() == chrono::NaiveTime::MIN {
                    json!(dt.date().to_string())
                } else {
                    json!(dt.format("%Y-%m-%dT%H:%M:%S").to_string())
                }
            })
            .unwrap_or_else(|| json!(d.as_f64())),
    }
}

pub fn read_xlsx(bytes: Vec<u8>, sheet: &str) -> Result<RecordBatch, String> {
    let mut wb = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| format!("classeur illisible : {e}"))?;
    let name = if sheet.is_empty() {
        wb.sheet_names().first().cloned().ok_or("classeur vide")?
    } else {
        sheet.to_string()
    };
    let range = wb
        .worksheet_range(&name)
        .map_err(|_| format!("feuille « {name} » introuvable"))?;
    let mut rows = range.rows();
    let header: Vec<String> = rows
        .next()
        .map(|h| {
            h.iter()
                .enumerate()
                .map(|(i, c)| match c {
                    Data::Empty => format!("colonne_{}", i + 1),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let items: Vec<Value> = rows
        .filter(|r| r.iter().any(|c| !matches!(c, Data::Empty)))
        .map(|r| {
            Value::Object(
                header
                    .iter()
                    .cloned()
                    .zip(r.iter().map(excel_cell))
                    .collect(),
            )
        })
        .collect();
    table_of(Value::Array(items))
}

pub async fn read(ctx: &Ctx, p: &Map<String, Value>) -> Result<RecordBatch, String> {
    let (name, bytes) = files::read(&ctx.db, ctx.workspace_id, &str_param(p, "file")).await?;
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "parquet" => data::from_parquet(bytes).map_err(|e| e.to_string()),
        "json" => {
            let doc: Value =
                serde_json::from_slice(&bytes).map_err(|e| format!("JSON invalide : {e}"))?;
            table_of(doc)
        }
        "xlsx" | "xlsm" | "xls" | "ods" => read_xlsx(bytes, &str_param(p, "sheet")),
        _ => {
            let text = String::from_utf8_lossy(&bytes);
            let text = text.trim_start_matches('\u{feff}');
            data::csv_to_batch(text, sniff_delimiter(text), true).map_err(|e| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_roundtrip() {
        let batch = data::json_to_batch(&json!([
            {"produit": "Clavier", "prix": 49.9, "stock": 3, "actif": true},
            {"produit": "Souris", "prix": null, "stock": 10, "actif": false}
        ]))
        .unwrap();
        let bytes = to_xlsx(&batch).unwrap();
        assert_eq!(&bytes[..2], b"PK");
        let back = data::batch_to_json(&read_xlsx(bytes, "").unwrap()).unwrap();
        assert_eq!(back[0]["produit"], "Clavier");
        assert_eq!(back[0]["prix"], 49.9);
        assert_eq!(back[1]["stock"], 10.0);
        assert_eq!(back[1]["actif"], false);
        assert!(back[1]["prix"].is_null());
    }

    #[test]
    fn names_and_delimiters() {
        assert!(render_name("ventes-{lignes}", 12, "csv").starts_with("ventes-12.csv"));
        assert_eq!(render_name("rapport.xlsx", 1, "xlsx"), "rapport.xlsx");
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3"), b';');
        assert_eq!(sniff_delimiter("a,b\n1,2"), b',');
        assert_eq!(sniff_delimiter("a\tb"), b'\t');
        assert_eq!(sniff_delimiter("seul"), b',');
    }
}
