//! Format d'échange entre nœuds (Parquet) et conversions JSON / CSV.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch, RecordBatchOptions, new_null_array};
use arrow::compute::{cast, concat_batches};
use arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use bytes::Bytes;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use serde::Serialize;
use serde_json::{Map, Value as Json};

use crate::error::{Result, fail};

// ---------------------------------------------------------------------------
// Parquet
// ---------------------------------------------------------------------------

/// Sérialise une table en Parquet (compression Snappy).
pub fn to_parquet(batch: &RecordBatch) -> Result<Vec<u8>> {
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_created_by(format!("mini8n {}", env!("CARGO_PKG_VERSION")))
        .build();
    let mut buf = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut buf, batch.schema(), Some(props))?;
    writer.write(batch)?;
    writer.close()?;
    Ok(buf)
}

/// Relit une table Parquet en un seul `RecordBatch`.
pub fn from_parquet(bytes: impl Into<Bytes>) -> Result<RecordBatch> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(bytes.into())?;
    let schema = builder.schema().clone();
    let rows = builder.metadata().file_metadata().num_rows() as usize;
    let batches = builder
        .build()?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if schema.fields().is_empty() {
        return empty_with_rows(schema, rows);
    }
    Ok(concat_batches(&schema, &batches)?)
}

fn empty_with_rows(schema: SchemaRef, rows: usize) -> Result<RecordBatch> {
    Ok(RecordBatch::try_new_with_options(
        schema,
        vec![],
        &RecordBatchOptions::new().with_row_count(Some(rows)),
    )?)
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// Aplatis les objets imbriqués : `{"a": {"b": 1}}` → `{"a.b": 1}`.
fn flatten_into(prefix: &str, obj: &Map<String, Json>, out: &mut Map<String, Json>) {
    for (k, v) in obj {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            Json::Object(inner) if !inner.is_empty() => flatten_into(&key, inner, out),
            _ => {
                out.insert(key, v.clone());
            }
        }
    }
}

fn stringify_nested(row: &mut Map<String, Json>) {
    for v in row.values_mut() {
        if matches!(v, Json::Array(_) | Json::Object(_)) {
            *v = Json::String(v.to_string());
        }
    }
}

/// Convertit un document JSON (tableau d'objets, ou objet seul) en table.
pub fn json_to_batch(doc: &Json) -> Result<RecordBatch> {
    let items: Vec<&Json> = match doc {
        Json::Array(a) => a.iter().collect(),
        Json::Null => vec![],
        other => vec![other],
    };
    let mut rows: Vec<Map<String, Json>> = items
        .into_iter()
        .map(|item| {
            let mut row = Map::new();
            match item {
                Json::Object(o) => flatten_into("", o, &mut row),
                scalar => {
                    row.insert("value".into(), scalar.clone());
                }
            }
            row
        })
        .collect();

    if rows.iter().all(Map::is_empty) {
        return empty_with_rows(Arc::new(Schema::empty()), rows.len());
    }
    match decode_rows(&rows) {
        Ok(b) => Ok(b),
        // Tableaux hétérogènes : on retombe sur du texte JSON.
        Err(_) => {
            rows.iter_mut().for_each(stringify_nested);
            decode_rows(&rows)
        }
    }
}

fn decode_rows(rows: &[Map<String, Json>]) -> Result<RecordBatch> {
    use arrow::json::ReaderBuilder;
    use arrow::json::reader::infer_json_schema_from_iterator;

    // Ordre des colonnes = ordre d'apparition des clés.
    let mut order: Vec<String> = Vec::new();
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for row in rows {
        for k in row.keys() {
            if seen.insert(k.as_str(), ()).is_none() {
                order.push(k.clone());
            }
        }
    }
    let values: Vec<Json> = rows.iter().cloned().map(Json::Object).collect();
    let inferred = infer_json_schema_from_iterator(values.iter().map(|v| Ok(v.clone())))?;
    let fields: Vec<Field> = order
        .iter()
        .filter_map(|name| inferred.field_with_name(name).ok())
        .map(|f| f.as_ref().clone().with_nullable(true))
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let mut decoder = ReaderBuilder::new(schema.clone())
        .with_coerce_primitive(true)
        .with_batch_size(values.len().max(1))
        .build_decoder()?;
    decoder.serialize(&values)?;
    let mut batches = Vec::new();
    while let Some(b) = decoder.flush()? {
        batches.push(b);
    }
    Ok(concat_batches(&schema, &batches)?)
}

/// Table → tableau JSON d'objets.
pub fn batch_to_json(batch: &RecordBatch) -> Result<Json> {
    use arrow::json::{WriterBuilder, writer::JsonArray};
    if batch.num_columns() == 0 {
        return Ok(Json::Array(vec![
            Json::Object(Map::new());
            batch.num_rows()
        ]));
    }
    let mut buf = Vec::new();
    {
        let mut w = WriterBuilder::new()
            .with_explicit_nulls(true)
            .build::<_, JsonArray>(&mut buf);
        w.write(batch)?;
        w.finish()?;
    }
    if buf.is_empty() {
        return Ok(Json::Array(vec![]));
    }
    Ok(serde_json::from_slice(&buf)?)
}

// ---------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------

pub fn csv_to_batch(text: &str, delimiter: u8, has_header: bool) -> Result<RecordBatch> {
    use arrow::csv::ReaderBuilder;
    use arrow::csv::reader::Format;

    if text.trim().is_empty() {
        return empty_with_rows(Arc::new(Schema::empty()), 0);
    }
    let format = Format::default()
        .with_header(has_header)
        .with_delimiter(delimiter);
    let (schema, _) = format.infer_schema(Cursor::new(text.as_bytes()), Some(10_000))?;
    let schema = Arc::new(schema);
    let reader = ReaderBuilder::new(schema.clone())
        .with_format(format)
        .build(Cursor::new(text.as_bytes()))?;
    let batches = reader.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(concat_batches(&schema, &batches)?)
}

/// Neutralise les cellules texte interprétées comme formules par les tableurs
/// (`=`, `+`, `-`, `@`, tabulation, retour chariot) en les préfixant d'une apostrophe.
fn neutralize_formulas(batch: &RecordBatch) -> Result<RecordBatch> {
    use arrow::array::{AsArray, StringArray};
    let dangerous = |s: &str| s.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let columns = batch
        .columns()
        .iter()
        .map(|col| -> Result<ArrayRef> {
            if !matches!(
                col.data_type(),
                DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View
            ) {
                return Ok(col.clone());
            }
            let text = cast(col, &DataType::Utf8)?;
            let text = text.as_string::<i32>();
            if !text.iter().flatten().any(dangerous) {
                return Ok(col.clone());
            }
            let safe: StringArray = text
                .iter()
                .map(|v| {
                    v.map(|s| {
                        if dangerous(s) {
                            format!("'{s}")
                        } else {
                            s.to_string()
                        }
                    })
                })
                .collect();
            Ok(Arc::new(safe))
        })
        .collect::<Result<Vec<_>>>()?;
    let schema = Schema::new(
        batch
            .schema()
            .fields()
            .iter()
            .zip(&columns)
            .map(|(f, c)| f.as_ref().clone().with_data_type(c.data_type().clone()))
            .collect::<Vec<_>>(),
    );
    Ok(RecordBatch::try_new(Arc::new(schema), columns)?)
}

pub fn batch_to_csv(batch: &RecordBatch) -> Result<String> {
    let batch = neutralize_formulas(batch)?;
    let mut buf = Vec::new();
    {
        let mut w = arrow::csv::WriterBuilder::new()
            .with_header(true)
            .build(&mut buf);
        w.write(&batch)?;
    }
    match String::from_utf8(buf) {
        Ok(s) => Ok(s),
        Err(_) => fail("CSV généré invalide (UTF-8)"),
    }
}

// ---------------------------------------------------------------------------
// Métadonnées / aperçu
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
}

pub fn columns(schema: &Schema) -> Vec<ColumnInfo> {
    schema
        .fields()
        .iter()
        .map(|f| ColumnInfo {
            name: f.name().clone(),
            data_type: f.data_type().to_string(),
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub columns: Vec<ColumnInfo>,
    pub rows: Json,
    pub total: usize,
    pub offset: usize,
}

pub fn preview(batch: &RecordBatch, offset: usize, limit: usize) -> Result<Preview> {
    let total = batch.num_rows();
    let offset = offset.min(total);
    let len = limit.min(total - offset);
    Ok(Preview {
        columns: columns(&batch.schema()),
        rows: batch_to_json(&batch.slice(offset, len))?,
        total,
        offset,
    })
}

// ---------------------------------------------------------------------------
// Concaténation (union des colonnes)
// ---------------------------------------------------------------------------

/// Empile des tables dont les colonnes peuvent différer : les colonnes manquantes
/// sont remplies de null, les types incompatibles sont convertis en texte.
pub fn append_batches(batches: &[RecordBatch]) -> Result<RecordBatch> {
    match batches {
        [] => return empty_with_rows(Arc::new(Schema::empty()), 0),
        [one] => return Ok(one.clone()),
        _ => {}
    }
    let mut names: Vec<String> = Vec::new();
    let mut types: HashMap<String, DataType> = HashMap::new();
    for b in batches {
        for f in b.schema().fields() {
            match types.get(f.name()) {
                None => {
                    names.push(f.name().clone());
                    types.insert(f.name().clone(), f.data_type().clone());
                }
                Some(t) if t != f.data_type() && t != &DataType::Null => {
                    let unified = if f.data_type() == &DataType::Null {
                        t.clone()
                    } else if t.is_numeric() && f.data_type().is_numeric() {
                        DataType::Float64
                    } else {
                        DataType::Utf8
                    };
                    types.insert(f.name().clone(), unified);
                }
                Some(DataType::Null) => {
                    types.insert(f.name().clone(), f.data_type().clone());
                }
                Some(_) => {}
            }
        }
    }
    if names.is_empty() {
        let rows = batches.iter().map(RecordBatch::num_rows).sum();
        return empty_with_rows(Arc::new(Schema::empty()), rows);
    }
    let schema = Arc::new(Schema::new(
        names
            .iter()
            .map(|n| Field::new(n, types[n].clone(), true))
            .collect::<Vec<_>>(),
    ));
    let aligned = batches
        .iter()
        .map(|b| -> Result<RecordBatch> {
            let cols = schema
                .fields()
                .iter()
                .map(|f| -> Result<ArrayRef> {
                    Ok(match b.column_by_name(f.name()) {
                        Some(c) if c.data_type() == f.data_type() => c.clone(),
                        Some(c) => cast(c, f.data_type())?,
                        None => new_null_array(f.data_type(), b.num_rows()),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(RecordBatch::try_new(schema.clone(), cols)?)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(concat_batches(&schema, &aligned)?)
}
