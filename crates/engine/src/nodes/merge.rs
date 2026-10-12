use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use arrow::array::{ArrayRef, RecordBatch, UInt32Array};
use arrow::compute::take;
use arrow::datatypes::{Field, Schema};

use super::{Params, column_indices};
use crate::data::append_batches;
use crate::error::{Result, fail};
use crate::value::RowAccessor;

pub fn merge(p: &Params, a: RecordBatch, b: RecordBatch) -> Result<RecordBatch> {
    match p.str_or("mode", "empiler").as_str() {
        "empiler" => append_batches(&[a, b]),
        "jointure interne" => join(p, a, b, false),
        "jointure gauche" => join(p, a, b, true),
        m => fail(format!("Mode de fusion inconnu : {m}")),
    }
}

fn join(p: &Params, a: RecordBatch, b: RecordBatch, keep_unmatched: bool) -> Result<RecordBatch> {
    let left_names = p.list("left_key");
    if left_names.is_empty() {
        return fail("Clé de A requise pour une jointure");
    }
    let right_names = match p.list("right_key") {
        r if r.is_empty() => left_names.clone(),
        r => r,
    };
    if left_names.len() != right_names.len() {
        return fail("Les clés de A et B doivent avoir le même nombre de colonnes");
    }
    let lk = column_indices(&a, &left_names)?;
    let rk = column_indices(&b, &right_names)?;

    let right_rows = RowAccessor::new(&b)?;
    let mut lookup: HashMap<String, Vec<u32>> = HashMap::new();
    for r in 0..b.num_rows() {
        if let Some(k) = right_rows.key(&rk, r) {
            lookup.entry(k).or_default().push(r as u32);
        }
    }

    let left_rows = RowAccessor::new(&a)?;
    let mut li: Vec<u32> = Vec::new();
    let mut ri: Vec<Option<u32>> = Vec::new();
    for l in 0..a.num_rows() {
        match left_rows.key(&lk, l).and_then(|k| lookup.get(&k)) {
            Some(matches) => {
                for &r in matches {
                    li.push(l as u32);
                    ri.push(Some(r));
                }
            }
            None if keep_unmatched => {
                li.push(l as u32);
                ri.push(None);
            }
            None => {}
        }
    }
    let li = UInt32Array::from(li);
    let ri = UInt32Array::from(ri);

    let mut fields: Vec<Field> = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    let a_schema = a.schema();
    for (i, f) in a_schema.fields().iter().enumerate() {
        names.insert(f.name().clone());
        fields.push(Field::new(f.name(), f.data_type().clone(), true));
        columns.push(take(a.column(i), &li, None)?);
    }
    let b_schema = b.schema();
    for (i, f) in b_schema.fields().iter().enumerate() {
        // La clé de B est redondante si elle porte le même nom que celle de A.
        if let Some(pos) = rk.iter().position(|&k| k == i)
            && right_names[pos] == left_names[pos]
        {
            continue;
        }
        let mut name = f.name().clone();
        while names.contains(&name) {
            name.push_str("_b");
        }
        names.insert(name.clone());
        fields.push(Field::new(&name, f.data_type().clone(), true));
        columns.push(take(b.column(i), &ri, None)?);
    }
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(fields)),
        columns,
    )?)
}
