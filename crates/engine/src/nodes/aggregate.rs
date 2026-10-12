use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use arrow::array::{ArrayRef, Int64Array, RecordBatch, UInt32Array};
use arrow::compute::take;
use arrow::datatypes::{Field, Schema};

use super::{Params, column_indices, split_list};
use crate::error::{Result, fail};
use crate::value::{Num, RowAccessor, Value, values_cmp, values_to_array};

#[derive(Debug, Clone, Copy, PartialEq)]
enum AggFn {
    Count,
    CountDistinct,
    Sum,
    Avg,
    Min,
    Max,
    First,
    Last,
}

struct AggSpec {
    func: AggFn,
    /// `None` pour `count(*)`.
    column: Option<usize>,
    alias: String,
}

fn parse_aggs(text: &str, batch: &RecordBatch) -> Result<Vec<AggSpec>> {
    let mut out = Vec::new();
    for item in split_list(text) {
        let (call, alias) = match item.to_ascii_lowercase().find(" as ") {
            Some(i) => (item[..i].trim(), Some(item[i + 4..].trim().to_string())),
            None => (item.as_str(), None),
        };
        let (Some(open), true) = (call.find('('), call.ends_with(')')) else {
            return fail(format!(
                "Agrégat invalide « {item} » (ex. sum(prix) as total)"
            ));
        };
        let fname = call[..open].trim().to_lowercase();
        let arg = call[open + 1..call.len() - 1].trim().trim_matches('`');
        let func = match fname.as_str() {
            "count" => AggFn::Count,
            "count_distinct" => AggFn::CountDistinct,
            "sum" => AggFn::Sum,
            "avg" | "mean" => AggFn::Avg,
            "min" => AggFn::Min,
            "max" => AggFn::Max,
            "first" => AggFn::First,
            "last" => AggFn::Last,
            f => return fail(format!("Fonction d'agrégat inconnue : {f}")),
        };
        let column = if arg == "*" || arg.is_empty() {
            if func != AggFn::Count {
                return fail(format!("{fname}() nécessite une colonne"));
            }
            None
        } else {
            Some(column_indices(batch, &[arg.to_string()])?[0])
        };
        let alias = alias.unwrap_or_else(|| match column {
            None => fname.clone(),
            Some(_) => format!("{fname}_{arg}"),
        });
        out.push(AggSpec {
            func,
            column,
            alias,
        });
    }
    if out.is_empty() {
        out.push(AggSpec {
            func: AggFn::Count,
            column: None,
            alias: "count".into(),
        });
    }
    Ok(out)
}

enum Acc {
    Count(i64),
    Distinct(HashSet<String>),
    Sum(Option<Num>),
    Avg(f64, i64),
    Pick(Option<Value>),
}

impl Acc {
    fn new(f: AggFn) -> Self {
        match f {
            AggFn::Count => Acc::Count(0),
            AggFn::CountDistinct => Acc::Distinct(HashSet::new()),
            AggFn::Sum => Acc::Sum(None),
            AggFn::Avg => Acc::Avg(0.0, 0),
            _ => Acc::Pick(None),
        }
    }

    fn update(&mut self, f: AggFn, v: Option<Value>) -> Result<()> {
        let v = match v {
            None => {
                // count(*)
                if let Acc::Count(n) = self {
                    *n += 1;
                }
                return Ok(());
            }
            Some(Value::Null) => return Ok(()),
            Some(v) => v,
        };
        let num = |v: &Value| {
            v.as_num().map_or_else(
                || fail(format!("Valeur non numérique « {} »", v.to_text())),
                Ok,
            )
        };
        match self {
            Acc::Count(n) => *n += 1,
            Acc::Distinct(set) => {
                set.insert(v.key().unwrap_or_default());
            }
            Acc::Sum(total) => {
                let x = num(&v)?;
                *total = Some(match (*total, x) {
                    (None, x) => x,
                    (Some(Num::I(a)), Num::I(b)) => {
                        a.checked_add(b).map_or(Num::F(a as f64 + b as f64), Num::I)
                    }
                    (Some(a), b) => Num::F(a.f64() + b.f64()),
                });
            }
            Acc::Avg(sum, n) => {
                *sum += num(&v)?.f64();
                *n += 1;
            }
            Acc::Pick(cur) => {
                let replace = match (f, &cur) {
                    (_, None) => true,
                    (AggFn::Last, _) => true,
                    (AggFn::Min, Some(c)) => values_cmp(&v, c) == Some(Ordering::Less),
                    (AggFn::Max, Some(c)) => values_cmp(&v, c) == Some(Ordering::Greater),
                    _ => false,
                };
                if replace {
                    *cur = Some(v);
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Value {
        match self {
            Acc::Count(n) => Value::Int(n),
            Acc::Distinct(s) => Value::Int(s.len() as i64),
            Acc::Sum(t) => t.map_or(Value::Null, Num::value),
            Acc::Avg(_, 0) => Value::Null,
            Acc::Avg(s, n) => Value::Float(s / n as f64),
            Acc::Pick(v) => v.unwrap_or(Value::Null),
        }
    }
}

pub fn aggregate(p: &Params, batch: RecordBatch) -> Result<RecordBatch> {
    let group_names = p.list("group_by");
    let group_cols = column_indices(&batch, &group_names)?;
    let aggs = parse_aggs(&p.str("aggregations"), &batch)?;
    let rows = RowAccessor::new(&batch)?;

    let mut index: HashMap<Vec<Option<String>>, usize> = HashMap::new();
    let mut first_rows: Vec<u32> = Vec::new();
    let mut accs: Vec<Vec<Acc>> = Vec::new();

    if group_cols.is_empty() {
        // Un seul groupe, même pour une table vide.
        first_rows.push(0);
        accs.push(aggs.iter().map(|a| Acc::new(a.func)).collect());
    }

    for r in 0..batch.num_rows() {
        let g = if group_cols.is_empty() {
            0
        } else {
            let key: Vec<Option<String>> =
                group_cols.iter().map(|&c| rows.get(c, r).key()).collect();
            *index.entry(key).or_insert_with(|| {
                first_rows.push(r as u32);
                accs.push(aggs.iter().map(|a| Acc::new(a.func)).collect());
                accs.len() - 1
            })
        };
        for (spec, acc) in aggs.iter().zip(accs[g].iter_mut()) {
            let v = spec.column.map(|c| rows.get(c, r));
            acc.update(spec.func, v)
                .map_err(|e| crate::error::EngineError::Msg(format!("{} : {e}", spec.alias)))?;
        }
    }

    let schema = batch.schema();
    let mut fields = Vec::new();
    let mut columns: Vec<ArrayRef> = Vec::new();
    let idx = UInt32Array::from(first_rows);
    for &c in &group_cols {
        fields.push(Field::new(
            schema.field(c).name(),
            schema.field(c).data_type().clone(),
            true,
        ));
        columns.push(take(batch.column(c), &idx, None)?);
    }
    let mut finished: Vec<Vec<Value>> = vec![Vec::with_capacity(accs.len()); aggs.len()];
    for group in accs {
        for (i, acc) in group.into_iter().enumerate() {
            finished[i].push(acc.finish());
        }
    }
    for (spec, values) in aggs.iter().zip(finished) {
        let array: ArrayRef = if matches!(spec.func, AggFn::Count | AggFn::CountDistinct) {
            Arc::new(
                values
                    .iter()
                    .map(|v| match v {
                        Value::Int(i) => Some(*i),
                        _ => None,
                    })
                    .collect::<Int64Array>(),
            )
        } else {
            values_to_array(&values)
        };
        if fields.iter().any(|f: &Field| f.name() == &spec.alias) {
            return fail(format!("Nom de colonne en double : « {} »", spec.alias));
        }
        fields.push(Field::new(&spec.alias, array.data_type().clone(), true));
        columns.push(array);
    }
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(fields)),
        columns,
    )?)
}
