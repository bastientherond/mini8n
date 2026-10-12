//! Évaluation ligne par ligne.

use std::cmp::Ordering;
use std::collections::HashSet;

use super::ExprError;
use super::compile::{Func, Ir, Lambda};
use super::docs;
use super::parser::{BinOp, Logic, UnOp};
use super::value::{self, V, compare, loose_eq, parse_date, parse_number, strict_eq};
use crate::value::RowAccessor;

/// Garde-fous : nombre d'opérations par évaluation et taille des textes / listes produits.
const MAX_STEPS: u64 = 2_000_000;
const MAX_TEXT: usize = 10 * 1024 * 1024;
const MAX_ITEMS: usize = 1_000_000;

pub struct Cx<'r, 'a> {
    pub rows: Option<&'r RowAccessor<'a>>,
    pub names: &'r [String],
    pub r: usize,
    pub offset: i32,
    pub locals: Vec<V>,
    pub steps: u64,
}

type R = Result<V, ExprError>;

fn fail<T>(msg: impl Into<String>) -> Result<T, ExprError> {
    Err(ExprError::msg(msg))
}

fn shown(v: &V, offset: i32) -> String {
    let t = match v {
        V::Str(s) => format!("\"{s}\""),
        other => other.to_text(offset),
    };
    if t.chars().count() > 40 {
        t.chars().take(40).collect::<String>() + "…"
    } else {
        t
    }
}

fn check_text(s: String) -> R {
    if s.len() > MAX_TEXT {
        return fail("Texte trop long (10 Mo max.)");
    }
    Ok(V::Str(s))
}

fn check_list(items: Vec<V>) -> R {
    if items.len() > MAX_ITEMS {
        return fail("Liste trop longue (1 000 000 éléments max.)");
    }
    Ok(V::list(items))
}

fn int_or_float(f: f64) -> V {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 {
        V::Int(f as i64)
    } else {
        V::Float(f)
    }
}

fn number(v: &V, what: &str, offset: i32) -> Result<Option<f64>, ExprError> {
    if v.is_null() {
        return Ok(None);
    }
    match v {
        V::List(_) | V::Obj(_) => fail(format!(
            "{what} : nombre attendu, {} reçu ({})",
            v.type_name(),
            shown(v, offset)
        )),
        _ => match v.as_f64() {
            Some(f) => Ok(Some(f)),
            None => fail(format!(
                "{what} : nombre attendu, {} reçu ({})",
                v.type_name(),
                shown(v, offset)
            )),
        },
    }
}

impl Cx<'_, '_> {
    fn tick(&mut self) -> Result<(), ExprError> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return fail("Expression trop coûteuse (trop d'opérations)");
        }
        Ok(())
    }

    pub fn eval(&mut self, ir: &Ir) -> R {
        self.tick()?;
        Ok(match ir {
            Ir::Const(v) => v.clone(),
            Ir::Col(i) => self
                .rows
                .map_or(V::Null, |rows| V::from(rows.get(*i, self.r))),
            Ir::Series(values) => values.get(self.r).cloned().unwrap_or(V::Null),
            Ir::Row => match self.rows {
                Some(rows) => V::obj(
                    self.names
                        .iter()
                        .enumerate()
                        .map(|(i, n)| (n.clone(), V::from(rows.get(i, self.r))))
                        .collect(),
                ),
                None => V::Null,
            },
            Ir::RowIndex => V::Int(self.r as i64),
            Ir::Local(i) => self.locals.get(*i).cloned().unwrap_or(V::Null),
            Ir::Template(parts) => {
                let mut out = String::new();
                for p in parts {
                    out.push_str(&self.eval(p)?.to_text(self.offset));
                    if out.len() > MAX_TEXT {
                        return fail("Texte trop long (10 Mo max.)");
                    }
                }
                V::Str(out)
            }
            Ir::Array(items) => check_list(
                items
                    .iter()
                    .map(|i| self.eval(i))
                    .collect::<Result<_, _>>()?,
            )?,
            Ir::Object(fields) => V::obj(
                fields
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.eval(v)?)))
                    .collect::<Result<_, ExprError>>()?,
            ),
            Ir::Member(obj, name) => {
                let v = self.eval(obj)?;
                self.member(v, name)?
            }
            Ir::Index(obj, idx) => {
                let v = self.eval(obj)?;
                let i = self.eval(idx)?;
                self.index(v, i)?
            }
            Ir::Method(recv, name, args) => {
                let v = self.eval(recv)?;
                self.method(v, name, args)?
            }
            Ir::Func(f, args) => self.func(*f, args)?,
            Ir::Lambda(_) => return fail("Fonction fléchée inattendue"),
            Ir::Unary(op, e) => {
                let v = self.eval(e)?;
                match op {
                    UnOp::Not => V::Bool(!v.truthy()),
                    UnOp::Plus => v.to_number(),
                    UnOp::Neg => match v {
                        V::Null => V::Null,
                        V::Int(i) => i.checked_neg().map_or(V::Float(-(i as f64)), V::Int),
                        other => match number(&other, "-", self.offset)? {
                            Some(f) => V::Float(-f),
                            None => V::Null,
                        },
                    },
                }
            }
            Ir::Logic(op, a, b) => {
                let a = self.eval(a)?;
                match op {
                    Logic::And if !a.truthy() => a,
                    Logic::Or if a.truthy() => a,
                    Logic::Nullish if !a.is_null() => a,
                    _ => self.eval(b)?,
                }
            }
            Ir::Cond(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.eval(a)?
                } else {
                    self.eval(b)?
                }
            }
            Ir::Bin(op, a, b) => {
                let a = self.eval(a)?;
                let b = self.eval(b)?;
                self.binary(*op, a, b)?
            }
        })
    }

    fn binary(&self, op: BinOp, a: V, b: V) -> R {
        let cmp = |pred: fn(Ordering) -> bool| V::Bool(compare(&a, &b).is_some_and(pred));
        Ok(match op {
            BinOp::Eq => V::Bool(loose_eq(&a, &b)),
            BinOp::Ne => V::Bool(!loose_eq(&a, &b)),
            BinOp::StrictEq => V::Bool(strict_eq(&a, &b)),
            BinOp::StrictNe => V::Bool(!strict_eq(&a, &b)),
            BinOp::Lt => cmp(|o| o == Ordering::Less),
            BinOp::Le => cmp(|o| o != Ordering::Greater),
            BinOp::Gt => cmp(|o| o == Ordering::Greater),
            BinOp::Ge => cmp(|o| o != Ordering::Less),
            _ => self.arith(op, a, b)?,
        })
    }

    fn arith(&self, op: BinOp, a: V, b: V) -> R {
        let off = self.offset;
        if op == BinOp::Add && (matches!(a, V::Str(_)) || matches!(b, V::Str(_))) {
            return check_text(a.to_text(off) + &b.to_text(off));
        }
        if let (V::Date(x), V::Date(y)) = (&a, &b)
            && op == BinOp::Sub
        {
            return Ok(V::Int(x - y));
        }
        if matches!(a, V::Date(_)) || matches!(b, V::Date(_)) {
            return fail("Pour décaler une date, utilisez .plus(n, 'days') ou .minus(n, 'hours')");
        }
        if matches!((&a, &b), (V::List(_), _) | (_, V::List(_))) && op == BinOp::Add {
            return fail("Pour assembler des listes, utilisez .concat()");
        }
        let sym = match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            _ => "**",
        };
        let (Some(x), Some(y)) = (number(&a, sym, off)?, number(&b, sym, off)?) else {
            return Ok(V::Null);
        };
        if let (V::Int(i), V::Int(j)) = (&a, &b) {
            let r = match op {
                BinOp::Add => i.checked_add(*j),
                BinOp::Sub => i.checked_sub(*j),
                BinOp::Mul => i.checked_mul(*j),
                BinOp::Mod if *j == 0 => return Ok(V::Null),
                BinOp::Mod => i.checked_rem(*j),
                BinOp::Pow => u32::try_from(*j).ok().and_then(|e| i.checked_pow(e)),
                _ => None,
            };
            if let Some(r) = r {
                return Ok(V::Int(r));
            }
        }
        Ok(match op {
            BinOp::Add => V::Float(x + y),
            BinOp::Sub => V::Float(x - y),
            BinOp::Mul => V::Float(x * y),
            BinOp::Div if y == 0.0 => V::Null,
            BinOp::Div => V::Float(x / y),
            BinOp::Mod if y == 0.0 => V::Null,
            BinOp::Mod => V::Float(x % y),
            _ => V::Float(x.powf(y)),
        })
    }

    fn member(&self, v: V, name: &str) -> R {
        Ok(match (&v, name) {
            (V::Null, _) => V::Null,
            (V::Obj(_), _) => v.field(name).cloned().unwrap_or(V::Null),
            (V::Str(s), "length") => V::Int(s.chars().count() as i64),
            (V::List(l), "length") => V::Int(l.len() as i64),
            (V::Date(ms), "monthLong") => {
                value::month_name(*ms, self.offset).map_or(V::Null, V::Str)
            }
            (V::Date(ms), "weekdayLong") => {
                value::day_name(*ms, self.offset).map_or(V::Null, V::Str)
            }
            (V::Date(ms), _) if value::DATE_PROPS.contains(&name) => {
                value::date_part(*ms, name, self.offset).unwrap_or(V::Null)
            }
            _ if docs::is_method(name) => {
                return fail(format!("{name} est une fonction : écrivez .{name}()"));
            }
            _ => {
                return fail(format!(
                    "Propriété « {name} » inexistante sur {} ({})",
                    article(&v),
                    shown(&v, self.offset)
                ));
            }
        })
    }

    fn index(&self, v: V, i: V) -> R {
        Ok(match (&v, &i) {
            (V::Null, _) | (_, V::Null) => V::Null,
            (V::List(l), _) => match i.as_f64() {
                Some(f) if f >= 0.0 => l.get(f as usize).cloned().unwrap_or(V::Null),
                _ => V::Null,
            },
            (V::Str(s), _) => match i.as_f64() {
                Some(f) if f >= 0.0 => s
                    .chars()
                    .nth(f as usize)
                    .map_or(V::Null, |c| V::Str(c.to_string())),
                _ => V::Null,
            },
            (V::Obj(_), _) => v.field(&i.to_text(self.offset)).cloned().unwrap_or(V::Null),
            _ => self.member(v, &i.to_text(self.offset))?,
        })
    }

    fn call_lambda(&mut self, f: &Lambda, args: &[V]) -> R {
        self.locals.truncate(f.base);
        for i in 0..f.params {
            self.locals.push(args.get(i).cloned().unwrap_or(V::Null));
        }
        let out = self.eval(&f.body);
        self.locals.truncate(f.base);
        out
    }

    fn func(&mut self, f: Func, args: &[Ir]) -> R {
        // Fonctions paresseuses.
        match f {
            Func::If => {
                return if self.eval(&args[0])?.truthy() {
                    self.eval(&args[1])
                } else {
                    self.eval(&args[2])
                };
            }
            Func::Coalesce => {
                for a in args {
                    let v = self.eval(a)?;
                    if !v.is_null() {
                        return Ok(v);
                    }
                }
                return Ok(V::Null);
            }
            Func::IfEmpty => {
                let v = self.eval(&args[0])?;
                return if is_empty(&v) {
                    self.eval(&args[1])
                } else {
                    Ok(v)
                };
            }
            _ => {}
        }
        let a: Vec<V> = args
            .iter()
            .map(|x| self.eval(x))
            .collect::<Result<_, _>>()?;
        let off = self.offset;
        let num = |i: usize, what: &str| number(&a[i], what, off);
        let text = |i: usize| (!a[i].is_null()).then(|| a[i].to_text(off));
        let map_num = |what: &str, f: fn(f64) -> f64| -> R {
            Ok(match num(0, what)? {
                None => V::Null,
                Some(x) => int_or_float(f(x)),
            })
        };
        Ok(match f {
            Func::Min | Func::Max => {
                let values: Vec<V> = if let [V::List(l)] = a.as_slice() {
                    l.iter().cloned().collect()
                } else {
                    a.clone()
                };
                extreme(&values, f == Func::Min)
            }
            Func::Number => a[0].to_number(),
            Func::String => text(0).map_or(V::Null, V::Str),
            Func::Boolean => V::Bool(a[0].truthy()),
            Func::ParseInt => match text(0) {
                None => V::Null,
                Some(s) => {
                    let radix = if a.len() > 1 {
                        num(1, "parseInt")?.unwrap_or(10.0) as u32
                    } else {
                        10
                    };
                    parse_int(&s, radix).map_or(V::Null, V::Int)
                }
            },
            Func::ParseFloat => text(0)
                .and_then(|s| parse_float(&s))
                .map_or(V::Null, int_or_float),
            Func::Round => map_num("Math.round", |x| (x + 0.5).floor())?,
            Func::Floor => map_num("floor", f64::floor)?,
            Func::Ceil => map_num("ceil", f64::ceil)?,
            Func::Trunc => map_num("Math.trunc", f64::trunc)?,
            Func::Abs => match &a[0] {
                V::Int(i) => V::Int(i.saturating_abs()),
                _ => map_num("abs", f64::abs)?,
            },
            Func::Sign => map_num("Math.sign", |x| if x == 0.0 { 0.0 } else { x.signum() })?,
            Func::Sqrt => map_num("sqrt", f64::sqrt)?,
            Func::Exp => map_num("Math.exp", f64::exp)?,
            Func::Log => map_num("Math.log", f64::ln)?,
            Func::Log10 => map_num("Math.log10", f64::log10)?,
            Func::Pow => self.arith(BinOp::Pow, a[0].clone(), a[1].clone())?,
            Func::Random => V::Float(random()),
            Func::Stringify => {
                let json = a[0].to_json(off);
                let pretty = a.get(2).is_some_and(|v| !v.is_null() && v.truthy());
                check_text(
                    if pretty {
                        serde_json::to_string_pretty(&json)
                    } else {
                        serde_json::to_string(&json)
                    }
                    .unwrap_or_default(),
                )?
            }
            Func::Parse => match text(0) {
                None => V::Null,
                Some(s) => match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(j) => V::from_json(&j),
                    Err(e) => return fail(format!("JSON.parse : texte JSON invalide ({e})")),
                },
            },
            Func::Keys | Func::Values | Func::Entries => object_parts(&a[0], f),
            Func::Upper => text(0).map_or(V::Null, |s| V::Str(s.to_uppercase())),
            Func::Lower => text(0).map_or(V::Null, |s| V::Str(s.to_lowercase())),
            Func::Trim => text(0).map_or(V::Null, |s| V::Str(s.trim().to_string())),
            Func::Len => match &a[0] {
                V::List(l) => V::Int(l.len() as i64),
                V::Null => V::Null,
                other => V::Int(other.to_text(off).chars().count() as i64),
            },
            Func::Concat => check_text(a.iter().map(|v| v.to_text(off)).collect())?,
            Func::Contains | Func::StartsWith | Func::EndsWith => match (text(0), text(1)) {
                (Some(s), Some(t)) => V::Bool(match f {
                    Func::Contains => s.contains(&t),
                    Func::StartsWith => s.starts_with(&t),
                    _ => s.ends_with(&t),
                }),
                _ => V::Null,
            },
            Func::ReplaceAll => match (text(0), text(1), text(2)) {
                (Some(s), Some(from), Some(to)) if !from.is_empty() => {
                    check_text(s.replace(&from, &to))?
                }
                (Some(s), Some(_), Some(_)) => V::Str(s),
                _ => V::Null,
            },
            Func::Substr => match text(0) {
                None => V::Null,
                Some(s) => {
                    let start = num(1, "substr")?.map_or(0, |n| n.max(0.0) as usize);
                    let count = if a.len() > 2 {
                        num(2, "substr")?.map_or(usize::MAX, |n| n.max(0.0) as usize)
                    } else {
                        usize::MAX
                    };
                    V::Str(s.chars().skip(start).take(count).collect())
                }
            },
            Func::RoundTo => {
                let digits = if a.len() > 1 {
                    num(1, "round")?.unwrap_or(0.0) as i32
                } else {
                    0
                };
                round_to(&a[0], digits, off)?
            }
            Func::IsNull => V::Bool(a[0].is_null()),
            Func::If | Func::Coalesce | Func::IfEmpty => unreachable!("évaluées paresseusement"),
        })
    }

    fn method(&mut self, recv: V, name: &str, args: &[Ir]) -> R {
        // Les fonctions fléchées sont passées sans être évaluées.
        let mut lambdas: Vec<Option<&Lambda>> = Vec::with_capacity(args.len());
        let mut a: Vec<V> = Vec::with_capacity(args.len());
        for arg in args {
            if let Ir::Lambda(l) = arg {
                lambdas.push(Some(l));
                a.push(V::Null);
            } else {
                lambdas.push(None);
                a.push(self.eval(arg)?);
            }
        }
        let off = self.offset;
        match (&recv, name) {
            (V::Null, "isEmpty") => return Ok(V::Bool(true)),
            (V::Null, "isNotEmpty") => return Ok(V::Bool(false)),
            (V::Null, _) => return Ok(V::Null),
            (_, "toString") => return Ok(V::Str(recv.to_text(off))),
            _ => {}
        }
        let out = match &recv {
            V::Str(s) => string_method(s, name, &a, off)?,
            V::Int(_) | V::Float(_) => number_method(&recv, name, &a, off)?,
            V::List(l) => self.list_method(l, name, &a, &lambdas)?,
            V::Obj(_) => object_method(&recv, name, &a)?,
            V::Date(ms) => date_method(*ms, name, &a, off)?,
            V::Bool(_) | V::Null => None,
        };
        match out {
            Some(v) => Ok(v),
            None => {
                let kinds: Vec<&str> = docs::DOCS
                    .iter()
                    .filter(|d| d.name == name && d.kind == "method")
                    .filter_map(|d| match d.on {
                        "string" => Some("un texte"),
                        "number" => Some("un nombre"),
                        "list" => Some("une liste"),
                        "object" => Some("un objet"),
                        "date" => Some("une date"),
                        _ => None,
                    })
                    .collect();
                let hint = match (&recv, name) {
                    (V::Str(_), _) if kinds.contains(&"une date") => {
                        " (convertissez d'abord : .toDate())"
                    }
                    (V::Str(_), _) if kinds.contains(&"un nombre") => {
                        " (convertissez d'abord : .toNumber())"
                    }
                    _ => "",
                };
                fail(format!(
                    ".{name}() s'applique à {}, pas à {} ({}){hint}",
                    kinds.join(" ou "),
                    article(&recv),
                    shown(&recv, off)
                ))
            }
        }
    }

    fn list_method(
        &mut self,
        l: &[V],
        name: &str,
        a: &[V],
        lambdas: &[Option<&Lambda>],
    ) -> Result<Option<V>, ExprError> {
        let off = self.offset;
        let lambda = |i: usize| -> Result<&Lambda, ExprError> {
            lambdas.get(i).copied().flatten().ok_or_else(|| {
                ExprError::msg(format!(".{name}() attend une fonction : .{name}(x => …)"))
            })
        };
        let int_arg = |i: usize, default: i64| -> Result<i64, ExprError> {
            match a.get(i) {
                None | Some(V::Null) => Ok(default),
                Some(v) => number(v, name, off).map(|n| n.map_or(default, |f| f as i64)),
            }
        };
        Ok(Some(match name {
            "join" => {
                let sep = a
                    .first()
                    .filter(|v| !v.is_null())
                    .map_or(",".to_string(), |v| v.to_text(off));
                check_text(
                    l.iter()
                        .map(|v| v.to_text(off))
                        .collect::<Vec<_>>()
                        .join(&sep),
                )?
            }
            "includes" => V::Bool(a.first().is_some_and(|x| l.iter().any(|v| strict_eq(v, x)))),
            "indexOf" => V::Int(
                a.first()
                    .and_then(|x| l.iter().position(|v| strict_eq(v, x)))
                    .map_or(-1, |p| p as i64),
            ),
            "at" => {
                let i = int_arg(0, 0)?;
                let idx = if i < 0 { l.len() as i64 + i } else { i };
                usize::try_from(idx)
                    .ok()
                    .and_then(|i| l.get(i))
                    .cloned()
                    .unwrap_or(V::Null)
            }
            "first" => l.first().cloned().unwrap_or(V::Null),
            "last" => l.last().cloned().unwrap_or(V::Null),
            "slice" => {
                let (s, e) = slice_bounds(
                    l.len(),
                    int_arg(0, 0)?,
                    a.get(1).map(|_| int_arg(1, l.len() as i64)).transpose()?,
                );
                V::list(l[s..e].to_vec())
            }
            "concat" => {
                let mut out = l.to_vec();
                for v in a {
                    match v {
                        V::List(more) => out.extend(more.iter().cloned()),
                        other => out.push(other.clone()),
                    }
                }
                check_list(out)?
            }
            "reverse" => V::list(l.iter().rev().cloned().collect()),
            "sort" => {
                let items = l.to_vec();
                let sorted = if let Some(Some(f)) = lambdas.first() {
                    let f = *f;
                    merge_sort(items, &mut |x, y| {
                        let r = self.call_lambda(f, &[x.clone(), y.clone()])?;
                        Ok(r.as_f64().map_or(Ordering::Equal, |n| {
                            n.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
                        }))
                    })?
                } else {
                    merge_sort(items, &mut |x, y| {
                        Ok(match (x.is_null(), y.is_null()) {
                            (true, true) => Ordering::Equal,
                            (true, false) => Ordering::Greater,
                            (false, true) => Ordering::Less,
                            _ => compare(x, y).unwrap_or(Ordering::Equal),
                        })
                    })?
                };
                V::list(sorted)
            }
            "unique" => {
                let mut seen = HashSet::new();
                V::list(
                    l.iter()
                        .filter(|v| seen.insert(v.to_json(off).to_string()))
                        .cloned()
                        .collect(),
                )
            }
            "compact" => V::list(
                l.iter()
                    .filter(|v| !v.is_null() && !matches!(v, V::Str(s) if s.is_empty()))
                    .cloned()
                    .collect(),
            ),
            "flat" => {
                let mut out = Vec::new();
                for v in l {
                    match v {
                        V::List(inner) => out.extend(inner.iter().cloned()),
                        other => out.push(other.clone()),
                    }
                }
                check_list(out)?
            }
            "sum" | "average" => {
                let mut total = 0f64;
                let mut all_int = true;
                let mut count = 0usize;
                for v in l.iter().filter(|v| !v.is_null()) {
                    let Some(f) = number(v, name, off)? else {
                        continue;
                    };
                    all_int &= matches!(v, V::Int(_));
                    total += f;
                    count += 1;
                }
                if name == "sum" {
                    if all_int {
                        int_or_float(total)
                    } else {
                        V::Float(total)
                    }
                } else if count == 0 {
                    V::Null
                } else {
                    int_or_float(total / count as f64)
                }
            }
            "min" | "max" => extreme(l, name == "min"),
            "isEmpty" => V::Bool(l.is_empty()),
            "isNotEmpty" => V::Bool(!l.is_empty()),
            "pluck" => {
                let field = a.first().map(|v| v.to_text(off)).unwrap_or_default();
                V::list(
                    l.iter()
                        .map(|v| {
                            v.field(&field)
                                .or_else(|| v.field("json").and_then(|j| j.field(&field)))
                                .cloned()
                                .unwrap_or(V::Null)
                        })
                        .collect(),
                )
            }
            "map" | "filter" | "find" | "findIndex" | "some" | "every" => {
                let f = lambda(0)?;
                let mut out = Vec::new();
                for (i, v) in l.iter().enumerate() {
                    let r = self.call_lambda(f, &[v.clone(), V::Int(i as i64)])?;
                    match name {
                        "map" => out.push(r),
                        "filter" if r.truthy() => out.push(v.clone()),
                        "find" if r.truthy() => return Ok(Some(v.clone())),
                        "findIndex" if r.truthy() => return Ok(Some(V::Int(i as i64))),
                        "some" if r.truthy() => return Ok(Some(V::Bool(true))),
                        "every" if !r.truthy() => return Ok(Some(V::Bool(false))),
                        _ => {}
                    }
                }
                match name {
                    "map" | "filter" => check_list(out)?,
                    "find" => V::Null,
                    "findIndex" => V::Int(-1),
                    "some" => V::Bool(false),
                    _ => V::Bool(true),
                }
            }
            "reduce" => {
                let f = lambda(0)?;
                let mut iter = l.iter().enumerate();
                let mut acc = match a.get(1) {
                    Some(init) => init.clone(),
                    None => match iter.next() {
                        Some((_, v)) => v.clone(),
                        None => {
                            return fail(
                                ".reduce() sur une liste vide nécessite une valeur initiale",
                            );
                        }
                    },
                };
                for (i, v) in iter {
                    acc = self.call_lambda(f, &[acc, v.clone(), V::Int(i as i64)])?;
                }
                acc
            }
            _ => return Ok(None),
        }))
    }
}

type Cmp<'c> = dyn FnMut(&V, &V) -> Result<Ordering, ExprError> + 'c;

/// Tri fusion stable : contrairement à `sort_by`, tolère un comparateur incohérent.
fn merge_sort(mut items: Vec<V>, cmp: &mut Cmp) -> Result<Vec<V>, ExprError> {
    if items.len() <= 1 {
        return Ok(items);
    }
    let right = items.split_off(items.len() / 2);
    let left = merge_sort(items, cmp)?;
    let right = merge_sort(right, cmp)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let mut l = left.into_iter().peekable();
    let mut r = right.into_iter().peekable();
    loop {
        let take_right = match (l.peek(), r.peek()) {
            (Some(a), Some(b)) => cmp(b, a)? == Ordering::Less,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => break,
        };
        out.extend(if take_right { r.next() } else { l.next() });
    }
    Ok(out)
}

fn article(v: &V) -> &'static str {
    match v {
        V::Null => "null",
        V::Bool(_) => "un booléen",
        V::Int(_) | V::Float(_) => "un nombre",
        V::Str(_) => "un texte",
        V::List(_) => "une liste",
        V::Obj(_) => "un objet",
        V::Date(_) => "une date",
    }
}

fn is_empty(v: &V) -> bool {
    match v {
        V::Null => true,
        V::Str(s) => s.is_empty(),
        V::List(l) => l.is_empty(),
        V::Obj(o) => o.is_empty(),
        _ => false,
    }
}

fn extreme(values: &[V], min: bool) -> V {
    let mut best: Option<&V> = None;
    for v in values.iter().filter(|v| !v.is_null()) {
        best = match best {
            None => Some(v),
            Some(b) => {
                let o = compare(v, b).unwrap_or(Ordering::Equal);
                let better = if min {
                    o == Ordering::Less
                } else {
                    o == Ordering::Greater
                };
                Some(if better { v } else { b })
            }
        };
    }
    best.cloned().unwrap_or(V::Null)
}

fn object_parts(v: &V, f: Func) -> V {
    let V::Obj(fields) = v else {
        return V::Null;
    };
    V::list(
        fields
            .iter()
            .map(|(k, v)| match f {
                Func::Keys => V::Str(k.clone()),
                Func::Values => v.clone(),
                _ => V::list(vec![V::Str(k.clone()), v.clone()]),
            })
            .collect(),
    )
}

fn round_to(v: &V, digits: i32, off: i32) -> R {
    Ok(match number(v, "round", off)? {
        None => V::Null,
        Some(_) if digits <= 0 && matches!(v, V::Int(_)) => v.clone(),
        Some(x) if digits <= 0 => int_or_float(x.round()),
        Some(x) => {
            let m = 10f64.powi(digits.min(15));
            V::Float((x * m).round() / m)
        }
    })
}

fn parse_int(s: &str, radix: u32) -> Option<i64> {
    let t = s.trim_start();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let radix = if (2..=36).contains(&radix) { radix } else { 10 };
    let end = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    let n = i64::from_str_radix(&digits[..end], radix).ok()?;
    Some(if neg { -n } else { n })
}

fn parse_float(s: &str) -> Option<f64> {
    let t = s.trim_start();
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_exp = false;
    let bytes = t.as_bytes();
    while end < bytes.len() {
        let c = bytes[end];
        let ok = c.is_ascii_digit()
            || (c == b'.' && !seen_dot && !seen_exp)
            || ((c == b'-' || c == b'+') && (end == 0 || matches!(bytes[end - 1], b'e' | b'E')))
            || ((c == b'e' || c == b'E') && !seen_exp && end > 0);
        if !ok {
            break;
        }
        seen_dot |= c == b'.';
        seen_exp |= c == b'e' || c == b'E';
        end += 1;
    }
    let mut candidate = &t[..end];
    while !candidate.is_empty() {
        if let Ok(f) = candidate.parse::<f64>() {
            return Some(f);
        }
        candidate = &candidate[..candidate.len() - 1];
    }
    None
}

fn slice_bounds(len: usize, start: i64, end: Option<i64>) -> (usize, usize) {
    let norm = |i: i64| -> usize {
        if i < 0 {
            (len as i64 + i).max(0) as usize
        } else {
            (i as usize).min(len)
        }
    };
    let s = norm(start);
    let e = end.map_or(len, norm);
    (s, e.max(s))
}

fn random() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Math::random()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static STATE: AtomicU64 = AtomicU64::new(0);
        let mut x = STATE.load(Ordering::Relaxed);
        if x == 0 {
            x = (crate::host::now_ms() as u64) ^ 0x9E37_79B9_7F4A_7C15;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        STATE.store(x, Ordering::Relaxed);
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn string_method(s: &str, name: &str, a: &[V], off: i32) -> Result<Option<V>, ExprError> {
    let text = |i: usize| a.get(i).map(|v| v.to_text(off)).unwrap_or_default();
    let int = |i: usize, default: i64| -> Result<i64, ExprError> {
        match a.get(i) {
            None | Some(V::Null) => Ok(default),
            Some(v) => number(v, name, off).map(|n| n.map_or(default, |f| f as i64)),
        }
    };
    let count = s.chars().count();
    Ok(Some(match name {
        "toUpperCase" => V::Str(s.to_uppercase()),
        "toLowerCase" => V::Str(s.to_lowercase()),
        "toTitleCase" => V::Str(
            s.split(' ')
                .map(|w| {
                    let mut c = w.chars();
                    c.next().map_or(String::new(), |f| {
                        f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()
                    })
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        "capitalize" => {
            let mut c = s.chars();
            V::Str(c.next().map_or(String::new(), |f| {
                f.to_uppercase().collect::<String>() + c.as_str()
            }))
        }
        "trim" => V::Str(s.trim().to_string()),
        "trimStart" => V::Str(s.trim_start().to_string()),
        "trimEnd" => V::Str(s.trim_end().to_string()),
        "includes" => V::Bool(s.contains(&text(0))),
        "startsWith" => V::Bool(s.starts_with(&text(0))),
        "endsWith" => V::Bool(s.ends_with(&text(0))),
        "indexOf" | "lastIndexOf" => {
            let needle = text(0);
            let found = if name == "indexOf" {
                s.find(&needle)
            } else {
                s.rfind(&needle)
            };
            V::Int(found.map_or(-1, |b| s[..b].chars().count() as i64))
        }
        "replace" => {
            let from = text(0);
            if from.is_empty() {
                V::Str(s.to_string())
            } else {
                check_text(s.replacen(&from, &text(1), 1))?
            }
        }
        "replaceAll" => {
            let from = text(0);
            if from.is_empty() {
                V::Str(s.to_string())
            } else {
                check_text(s.replace(&from, &text(1)))?
            }
        }
        "slice" => {
            let (st, en) = slice_bounds(
                count,
                int(0, 0)?,
                a.get(1).map(|_| int(1, count as i64)).transpose()?,
            );
            V::Str(s.chars().skip(st).take(en - st).collect())
        }
        "substring" => {
            let clamp = |i: i64| i.clamp(0, count as i64) as usize;
            let (x, y) = (clamp(int(0, 0)?), clamp(int(1, count as i64)?));
            let (st, en) = if x <= y { (x, y) } else { (y, x) };
            V::Str(s.chars().skip(st).take(en - st).collect())
        }
        "split" => {
            let sep = text(0);
            let parts: Vec<V> = if a.is_empty() || a[0].is_null() {
                vec![V::Str(s.to_string())]
            } else if sep.is_empty() {
                s.chars().map(|c| V::Str(c.to_string())).collect()
            } else {
                s.split(sep.as_str())
                    .map(|p| V::Str(p.to_string()))
                    .collect()
            };
            check_list(parts)?
        }
        "padStart" | "padEnd" => {
            let target = int(0, 0)?.clamp(0, 1_000_000) as usize;
            let fill = if a.len() > 1 { text(1) } else { " ".into() };
            if target <= count || fill.is_empty() {
                V::Str(s.to_string())
            } else {
                let pad: String = fill.chars().cycle().take(target - count).collect();
                V::Str(if name == "padStart" {
                    pad + s
                } else {
                    s.to_string() + &pad
                })
            }
        }
        "repeat" => {
            let n = int(0, 0)?;
            if n < 0 {
                return fail(".repeat() : nombre négatif");
            }
            if s.len().saturating_mul(n as usize) > MAX_TEXT {
                return fail("Texte trop long (10 Mo max.)");
            }
            V::Str(s.repeat(n as usize))
        }
        "charAt" => {
            let i = int(0, 0)?;
            V::Str(if i < 0 {
                String::new()
            } else {
                s.chars()
                    .nth(i as usize)
                    .map(String::from)
                    .unwrap_or_default()
            })
        }
        "at" => {
            let i = int(0, 0)?;
            let idx = if i < 0 { count as i64 + i } else { i };
            usize::try_from(idx)
                .ok()
                .and_then(|i| s.chars().nth(i))
                .map_or(V::Null, |c| V::Str(c.to_string()))
        }
        "concat" => {
            check_text(s.to_string() + &a.iter().map(|v| v.to_text(off)).collect::<String>())?
        }
        "isEmpty" => V::Bool(s.is_empty()),
        "isNotEmpty" => V::Bool(!s.is_empty()),
        "isEmail" => V::Bool(is_email(s)),
        "isNumeric" => V::Bool(parse_number(s).is_some()),
        "toNumber" => parse_number(s).unwrap_or(V::Null),
        "toDate" => parse_date(s, off).map_or(V::Null, V::Date),
        "urlEncode" => V::Str(url_encode(s)),
        "urlDecode" => V::Str(url_decode(s)),
        "length" => V::Int(count as i64),
        _ => return Ok(None),
    }))
}

fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(h);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Format français : « 1 234 567,891 » (3 décimales max., comme toLocaleString).
fn french_number(x: f64) -> String {
    if !x.is_finite() {
        return x.to_string();
    }
    let rounded = (x * 1000.0).round() / 1000.0;
    let text = format!("{:.3}", rounded.abs());
    let (int_part, frac) = text.split_once('.').unwrap_or((&text, ""));
    let frac = frac.trim_end_matches('0');
    let mut grouped = String::new();
    for (i, c) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            grouped.push(' ');
        }
        grouped.push(c);
    }
    let sign = if rounded < 0.0 { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped},{frac}")
    }
}

fn number_method(v: &V, name: &str, a: &[V], off: i32) -> Result<Option<V>, ExprError> {
    let x = v.as_f64().unwrap_or(f64::NAN);
    let int = |i: usize, default: i64| -> Result<i64, ExprError> {
        match a.get(i) {
            None | Some(V::Null) => Ok(default),
            Some(v) => number(v, name, off).map(|n| n.map_or(default, |f| f as i64)),
        }
    };
    Ok(Some(match name {
        "toFixed" => {
            let d = int(0, 0)?.clamp(0, 20) as usize;
            V::Str(format!("{x:.d$}"))
        }
        "round" => round_to(v, int(0, 0)? as i32, off)?,
        "floor" => int_or_float(x.floor()),
        "ceil" => int_or_float(x.ceil()),
        "abs" => match v {
            V::Int(i) => V::Int(i.saturating_abs()),
            _ => V::Float(x.abs()),
        },
        "isEven" => V::Bool(x.fract() == 0.0 && (x as i64) % 2 == 0),
        "isOdd" => V::Bool(x.fract() == 0.0 && (x as i64) % 2 != 0),
        "isInteger" => V::Bool(x.is_finite() && x.fract() == 0.0),
        "toLocaleString" => V::Str(french_number(x)),
        "toDate" => V::Date(x as i64),
        _ => return Ok(None),
    }))
}

fn object_method(v: &V, name: &str, a: &[V]) -> Result<Option<V>, ExprError> {
    let V::Obj(fields) = v else {
        return Ok(None);
    };
    Ok(Some(match name {
        "keys" => object_parts(v, Func::Keys),
        "values" => object_parts(v, Func::Values),
        "hasField" => {
            let n = a.first().map(|v| v.to_text(0)).unwrap_or_default();
            V::Bool(fields.iter().any(|(k, _)| *k == n))
        }
        "isEmpty" => V::Bool(fields.is_empty()),
        "isNotEmpty" => V::Bool(!fields.is_empty()),
        _ => return Ok(None),
    }))
}

fn to_ms(v: &V, off: i32) -> Option<i64> {
    match v {
        V::Date(ms) => Some(*ms),
        V::Int(i) => Some(*i),
        V::Float(f) => Some(*f as i64),
        V::Str(s) => parse_date(s, off),
        _ => None,
    }
}

fn date_method(ms: i64, name: &str, a: &[V], off: i32) -> Result<Option<V>, ExprError> {
    let unit_arg = |i: usize, default: value::Unit| -> Result<value::Unit, ExprError> {
        match a.get(i) {
            None | Some(V::Null) => Ok(default),
            Some(v) => {
                let t = v.to_text(off);
                value::unit(&t).ok_or_else(|| {
                    ExprError::msg(format!(
                        "Unité inconnue « {t} » (years, months, weeks, days, hours, minutes, seconds)"
                    ))
                })
            }
        }
    };
    let other = |i: usize| -> Result<i64, ExprError> {
        a.get(i)
            .and_then(|v| to_ms(v, off))
            .ok_or_else(|| ExprError::msg(format!(".{name}() attend une date")))
    };
    let invalid = || ExprError::msg("Date hors limites");
    Ok(Some(match name {
        "format" => {
            let pattern = a
                .first()
                .map(|v| v.to_text(off))
                .unwrap_or_else(|| "dd/MM/yyyy".into());
            V::Str(value::format(ms, &pattern, off).ok_or_else(invalid)?)
        }
        "toISO" => V::Str(value::iso(ms, off)),
        "toISODate" => V::Str(value::format(ms, "yyyy-MM-dd", off).ok_or_else(invalid)?),
        "toMillis" => V::Int(ms),
        "toSeconds" => int_or_float(ms as f64 / 1000.0),
        "toLocaleString" => V::Str(value::format(ms, "dd/MM/yyyy HH:mm", off).ok_or_else(invalid)?),
        "plus" | "minus" => {
            let sign = if name == "plus" { 1.0 } else { -1.0 };
            let mut out = ms;
            match a.first() {
                // Durée façon Luxon : .plus({ days: 1, hours: 2 })
                Some(V::Obj(fields)) => {
                    for (k, v) in fields.iter() {
                        let u = value::unit(k)
                            .ok_or_else(|| ExprError::msg(format!("Unité inconnue « {k} »")))?;
                        let n = number(v, name, off)?.unwrap_or(0.0);
                        out = value::plus(out, sign * n, u, off).ok_or_else(invalid)?;
                    }
                }
                Some(v) => {
                    let n = number(v, name, off)?.unwrap_or(0.0);
                    let u = unit_arg(1, value::Unit::Millisecond)?;
                    out = value::plus(out, sign * n, u, off).ok_or_else(invalid)?;
                }
                None => return fail(format!(".{name}() attend une durée : .{name}(1, 'days')")),
            }
            V::Date(out)
        }
        "startOf" => {
            V::Date(value::start_of(ms, unit_arg(0, value::Unit::Day)?, off).ok_or_else(invalid)?)
        }
        "endOf" => {
            V::Date(value::end_of(ms, unit_arg(0, value::Unit::Day)?, off).ok_or_else(invalid)?)
        }
        "diff" => {
            let d = value::diff(ms, other(0)?, unit_arg(1, value::Unit::Day)?, off);
            int_or_float((d * 1e6).round() / 1e6)
        }
        "isBefore" => V::Bool(ms < other(0)?),
        "isAfter" => V::Bool(ms > other(0)?),
        _ => return Ok(None),
    }))
}
