//! Compilation : arbre syntaxique → représentation évaluable, en résolvant les
//! colonnes (`$json.prix`), les autres nœuds (`$('Clients')`) et les fonctions.

use std::sync::Arc;

use arrow::array::RecordBatch;

use super::parser::{Ast, AstKind, BinOp, Logic, TplPart, UnOp};
use super::value::{self, V};
use super::{ExprError, Scope, Span, docs};
use crate::value::RowAccessor;

/// Lignes au-delà desquelles `$input.all()` est refusé (mémoire).
const MAX_ALL_ROWS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func {
    // $…
    If,
    IfEmpty,
    Min,
    Max,
    // conversions
    Number,
    String,
    Boolean,
    ParseInt,
    ParseFloat,
    // Math
    Round,
    Floor,
    Ceil,
    Trunc,
    Abs,
    Sign,
    Sqrt,
    Pow,
    Exp,
    Log,
    Log10,
    Random,
    // JSON / Object
    Stringify,
    Parse,
    Keys,
    Values,
    Entries,
    // anciennes fonctions
    Upper,
    Lower,
    Trim,
    Len,
    Concat,
    Contains,
    StartsWith,
    EndsWith,
    ReplaceAll,
    Substr,
    RoundTo,
    Coalesce,
    IsNull,
}

#[derive(Debug, Clone)]
pub struct Lambda {
    /// Profondeur de la pile des paramètres à la définition.
    pub base: usize,
    pub params: usize,
    pub body: Ir,
}

#[derive(Debug, Clone)]
pub enum Ir {
    Const(V),
    /// Colonne de l'entrée, ligne en cours.
    Col(usize),
    /// Valeurs d'une colonne d'un autre nœud, alignées sur la ligne en cours.
    Series(Arc<Vec<V>>),
    /// Ligne en cours de l'entrée, comme objet.
    Row,
    RowIndex,
    Local(usize),
    Template(Vec<Ir>),
    Array(Vec<Ir>),
    Object(Vec<(String, Ir)>),
    Member(Box<Ir>, String),
    Index(Box<Ir>, Box<Ir>),
    Method(Box<Ir>, String, Vec<Ir>),
    Func(Func, Vec<Ir>),
    Lambda(Arc<Lambda>),
    Unary(UnOp, Box<Ir>),
    Bin(BinOp, Box<Ir>, Box<Ir>),
    Logic(Logic, Box<Ir>, Box<Ir>),
    Cond(Box<Ir>, Box<Ir>, Box<Ir>),
}

/// Table source d'une référence de ligne.
#[derive(Clone)]
enum Table {
    Input,
    Node(Arc<RecordBatch>),
}

#[derive(Clone, Copy)]
enum RowSel {
    Current,
    Fixed(usize),
}

#[derive(Clone)]
struct RowSrc {
    table: Table,
    row: RowSel,
}

#[derive(Clone, Copy, PartialEq)]
enum Ns {
    Math,
    Json,
    Object,
    /// `$node["Nom"]` (ancienne syntaxe de n8n).
    Node,
}

/// Valeur intermédiaire pendant la compilation.
enum Cv {
    Ir(Ir),
    Ns(Ns),
    /// `$input` ou `$('Nom')`.
    Source(Table),
    /// Élément `{ json }` d'une ligne.
    Item(RowSrc),
    /// `$json`, ou un préfixe de colonne aplatie (`$json.adresse` pour `adresse.ville`).
    Path(RowSrc, String),
}

pub struct Compiler<'s, 'a> {
    pub scope: &'s Scope<'a>,
    locals: Vec<String>,
}

fn err<T>(msg: impl Into<String>, span: Span) -> Result<T, ExprError> {
    Err(ExprError::at(msg, span))
}

/// Distance d'édition (suggestions « vouliez-vous dire »).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(ca != cb))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}

pub fn suggest<'n>(name: &str, candidates: impl IntoIterator<Item = &'n str>) -> Option<&'n str> {
    candidates
        .into_iter()
        .map(|c| (distance(name, c), c))
        .filter(|(d, c)| *d <= 2.max(c.chars().count() / 3) && *d < name.chars().count().max(2))
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Accès conseillé à une colonne (`$json.prix` ou `$json["code postal"]`).
pub fn column_ref(name: &str) -> String {
    if name.split('.').all(is_ident) {
        format!("$json.{name}")
    } else {
        format!("$json[{}]", serde_json::to_string(name).unwrap_or_default())
    }
}

/// Lignes d'une table converties en objets (pour `.first()`, `.all()`…).
fn row_object(rows: &RowAccessor, names: &[String], r: usize) -> V {
    V::obj(
        names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), V::from(rows.get(i, r))))
            .collect(),
    )
}

fn item(json: V) -> V {
    V::obj(vec![("json".into(), json)])
}

fn names_of(batch: &RecordBatch) -> Vec<String> {
    batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect()
}

impl<'s, 'a> Compiler<'s, 'a> {
    pub fn new(scope: &'s Scope<'a>) -> Self {
        Self {
            scope,
            locals: Vec::new(),
        }
    }

    pub fn compile(&mut self, ast: &Ast) -> Result<Ir, ExprError> {
        let cv = self.cv(ast)?;
        self.value(cv, ast.span)
    }

    fn batch<'t>(&'t self, table: &'t Table) -> Option<&'t RecordBatch> {
        match table {
            Table::Input => self.scope.input,
            Table::Node(b) => Some(b),
        }
    }

    /// Convertit une valeur intermédiaire en valeur évaluable.
    fn value(&self, cv: Cv, span: Span) -> Result<Ir, ExprError> {
        match cv {
            Cv::Ir(ir) => Ok(ir),
            Cv::Ns(ns) => err(
                match ns {
                    Ns::Math => "Math s'utilise avec une fonction : Math.round(x), Math.max(a, b)…",
                    Ns::Json => {
                        "JSON s'utilise avec une fonction : JSON.stringify(x) ou JSON.parse(texte)"
                    }
                    Ns::Object => "Object s'utilise avec une fonction : Object.keys(x)…",
                    Ns::Node => "Écrivez $node[\"Nom du nœud\"].json.colonne",
                },
                span,
            ),
            Cv::Source(_) => err(
                "Choisissez une ligne : .first(), .last(), .all() ou .item (puis .json.colonne)",
                span,
            ),
            Cv::Item(src) => Ok(Ir::Object(vec![(
                "json".into(),
                self.value(Cv::Path(src, String::new()), span)?,
            )])),
            Cv::Path(src, prefix) => {
                if prefix.is_empty() {
                    self.whole_row(&src, span)
                } else {
                    self.prefix_object(&src, &prefix, span)
                }
            }
        }
    }

    fn whole_row(&self, src: &RowSrc, span: Span) -> Result<Ir, ExprError> {
        let Some(batch) = self.batch(&src.table) else {
            return self.no_input(span);
        };
        let names = names_of(batch);
        match (src.row, &src.table) {
            (RowSel::Current, Table::Input) => Ok(Ir::Row),
            (RowSel::Current, Table::Node(_)) => {
                let rows =
                    RowAccessor::new(batch).map_err(|e| ExprError::at(e.to_string(), span))?;
                let n = batch.num_rows().min(MAX_ALL_ROWS);
                Ok(Ir::Series(Arc::new(
                    (0..n).map(|r| row_object(&rows, &names, r)).collect(),
                )))
            }
            (RowSel::Fixed(r), _) => {
                if r >= batch.num_rows() {
                    return Ok(Ir::Const(V::Null));
                }
                let rows =
                    RowAccessor::new(batch).map_err(|e| ExprError::at(e.to_string(), span))?;
                Ok(Ir::Const(row_object(&rows, &names, r)))
            }
        }
    }

    /// Objet reconstitué depuis des colonnes aplaties : `$json.ville` → `{ cp, nom }`.
    fn prefix_object(&self, src: &RowSrc, prefix: &str, span: Span) -> Result<Ir, ExprError> {
        let Some(batch) = self.batch(&src.table) else {
            return self.no_input(span);
        };
        let dotted = format!("{prefix}.");
        let mut keys: Vec<String> = Vec::new();
        for f in batch.schema().fields() {
            if let Some(rest) = f.name().strip_prefix(&dotted) {
                let key = rest.split('.').next().unwrap_or(rest).to_string();
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let mut fields = Vec::with_capacity(keys.len());
        for key in keys {
            let cv = self.path_member(src.clone(), prefix, &key, span)?;
            fields.push((key, self.value(cv, span)?));
        }
        Ok(Ir::Object(fields))
    }

    fn no_input<T>(&self, span: Span) -> Result<T, ExprError> {
        err(
            "Pas de données en entrée ici : $json n'est pas disponible (exécutez d'abord les nœuds précédents)",
            span,
        )
    }

    fn unknown_column<T>(&self, src: &RowSrc, name: &str, span: Span) -> Result<T, ExprError> {
        let Some(batch) = self.batch(&src.table) else {
            return self.no_input(span);
        };
        let names = names_of(batch);
        let mut msg = format!("Colonne inconnue « {name} »");
        if let Some(s) = suggest(name, names.iter().map(String::as_str)) {
            msg.push_str(&format!(" — vouliez-vous dire {} ?", column_ref(s)));
        } else if names.is_empty() {
            msg.push_str(" (l'entrée n'a aucune colonne)");
        } else {
            let list: Vec<_> = names.iter().take(12).map(String::as_str).collect();
            msg.push_str(&format!(
                " (disponibles : {}{})",
                list.join(", "),
                if names.len() > 12 { ", …" } else { "" }
            ));
        }
        err(msg, span)
    }

    /// Accès à un champ d'une ligne, en gérant les colonnes aplaties (`adresse.ville`).
    fn path_member(
        &self,
        src: RowSrc,
        prefix: &str,
        name: &str,
        span: Span,
    ) -> Result<Cv, ExprError> {
        let Some(batch) = self.batch(&src.table) else {
            return self.no_input(span);
        };
        let full = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        let schema = batch.schema();
        if let Ok(i) = schema.index_of(&full) {
            return Ok(Cv::Ir(match (src.row, &src.table) {
                (RowSel::Current, Table::Input) => Ir::Col(i),
                (RowSel::Current, Table::Node(b)) => {
                    let rows =
                        RowAccessor::new(b).map_err(|e| ExprError::at(e.to_string(), span))?;
                    let n = b.num_rows().min(MAX_ALL_ROWS);
                    Ir::Series(Arc::new((0..n).map(|r| V::from(rows.get(i, r))).collect()))
                }
                (RowSel::Fixed(r), _) => {
                    if r >= batch.num_rows() {
                        Ir::Const(V::Null)
                    } else {
                        let rows = RowAccessor::new(batch)
                            .map_err(|e| ExprError::at(e.to_string(), span))?;
                        Ir::Const(V::from(rows.get(i, r)))
                    }
                }
            }));
        }
        let dotted = format!("{full}.");
        if schema
            .fields()
            .iter()
            .any(|f| f.name().starts_with(&dotted))
        {
            return Ok(Cv::Path(src, full));
        }
        self.unknown_column(&src, &full, span)
    }

    fn input_src(&self, span: Span) -> Result<RowSrc, ExprError> {
        if self.scope.input.is_none() {
            return self.no_input(span);
        }
        Ok(RowSrc {
            table: Table::Input,
            row: RowSel::Current,
        })
    }

    fn node_table(&self, name: &str, span: Span) -> Result<Table, ExprError> {
        if let Some(lookup) = self.scope.nodes
            && let Some(batch) = lookup(name)
        {
            return Ok(Table::Node(Arc::new(batch)));
        }
        if self.scope.nodes.is_none() {
            return err("$('…') n'est pas disponible dans ce nœud", span);
        }
        let mut msg = format!("Nœud « {name} » introuvable ou pas encore exécuté");
        if let Some(s) = suggest(name, self.scope.node_names.iter().map(String::as_str)) {
            msg.push_str(&format!(" — vouliez-vous dire $('{s}') ?"));
        }
        err(msg, span)
    }

    fn ident(&self, name: &str, span: Span) -> Result<Cv, ExprError> {
        if let Some(i) = self.locals.iter().rposition(|l| l == name) {
            return Ok(Cv::Ir(Ir::Local(i)));
        }
        Ok(match name {
            "$json" => Cv::Path(self.input_src(span)?, String::new()),
            "$input" => {
                self.input_src(span)?;
                Cv::Source(Table::Input)
            }
            "$node" => Cv::Ns(Ns::Node),
            "$now" => Cv::Ir(Ir::Const(V::Date(self.scope.now_ms))),
            "$today" => Cv::Ir(Ir::Const(V::Date(
                value::start_of(self.scope.now_ms, value::Unit::Day, self.scope.offset_min)
                    .unwrap_or(self.scope.now_ms),
            ))),
            "$index" | "$itemIndex" => Cv::Ir(Ir::RowIndex),
            "Math" => Cv::Ns(Ns::Math),
            "JSON" => Cv::Ns(Ns::Json),
            "Object" => Cv::Ns(Ns::Object),
            "Infinity" => Cv::Ir(Ir::Const(V::Float(f64::INFINITY))),
            "NaN" => Cv::Ir(Ir::Const(V::Float(f64::NAN))),
            _ => {
                if let Some((_, v)) = self.scope.vars.iter().find(|(k, _)| k == name) {
                    return Ok(Cv::Ir(Ir::Const(V::from(v.clone()))));
                }
                // Ancienne syntaxe : nom de colonne nu (`prix > 15`).
                if let Some(input) = self.scope.input {
                    let schema = input.schema();
                    let dotted = format!("{name}.");
                    if schema.index_of(name).is_ok()
                        || schema
                            .fields()
                            .iter()
                            .any(|f| f.name().starts_with(&dotted))
                    {
                        return self.path_member(self.input_src(span)?, "", name, span);
                    }
                }
                return self.undefined(name, span);
            }
        })
    }

    fn undefined<T>(&self, name: &str, span: Span) -> Result<T, ExprError> {
        let mut msg = format!("« {name} » n'est pas défini");
        let columns = self.scope.input.map(names_of).unwrap_or_default();
        if let Some(c) = suggest(name, columns.iter().map(String::as_str)) {
            msg.push_str(&format!(" — vouliez-vous dire {} ?", column_ref(c)));
        } else if let Some(v) = suggest(
            name,
            [
                "$json", "$input", "$now", "$today", "$index", "Math", "JSON",
            ],
        ) {
            msg.push_str(&format!(" — vouliez-vous dire {v} ?"));
        } else if !name.starts_with('$') && !columns.is_empty() {
            msg.push_str(" (pour une colonne, écrivez $json.nom_de_colonne)");
        }
        err(msg, span)
    }

    fn cv(&mut self, ast: &Ast) -> Result<Cv, ExprError> {
        let span = ast.span;
        let ir = match &ast.kind {
            AstKind::Null => Ir::Const(V::Null),
            AstKind::Bool(b) => Ir::Const(V::Bool(*b)),
            AstKind::Int(i) => Ir::Const(V::Int(*i)),
            AstKind::Float(f) => Ir::Const(V::Float(*f)),
            AstKind::Str(s) => Ir::Const(V::Str(s.clone())),
            AstKind::Template(parts) => Ir::Template(
                parts
                    .iter()
                    .map(|p| match p {
                        TplPart::Text(t) => Ok(Ir::Const(V::Str(t.clone()))),
                        TplPart::Expr(e) => self.compile(e),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            AstKind::Ident(name) => return self.ident(name, span),
            AstKind::Array(items) => Ir::Array(
                items
                    .iter()
                    .map(|i| self.compile(i))
                    .collect::<Result<_, _>>()?,
            ),
            AstKind::Object(fields) => Ir::Object(
                fields
                    .iter()
                    .map(|(k, v)| Ok((k.clone(), self.compile(v)?)))
                    .collect::<Result<_, ExprError>>()?,
            ),
            AstKind::Member(obj, name, name_span) => {
                let base = self.cv(obj)?;
                return self.member(base, name, *name_span);
            }
            AstKind::Index(obj, index) => {
                let base = self.cv(obj)?;
                let index_ir = self.compile(index)?;
                if let Ir::Const(V::Str(name)) = &index_ir
                    && !matches!(base, Cv::Ir(_))
                {
                    return self.member(base, name, index.span);
                }
                if let Ir::Const(V::Int(i)) = &index_ir
                    && let Cv::Source(table) = &base
                {
                    // `$input[0]` : ancienne écriture de `$input.all()[0]`.
                    return Ok(Cv::Item(RowSrc {
                        table: table.clone(),
                        row: RowSel::Fixed((*i).max(0) as usize),
                    }));
                }
                let base = self.value(base, obj.span)?;
                Ir::Index(Box::new(base), Box::new(index_ir))
            }
            AstKind::Call(callee, args) => return self.call(callee, args, span),
            AstKind::Arrow(..) => {
                return err(
                    "Une fonction fléchée ne s'utilise qu'en argument : liste.map(x => x * 2)",
                    span,
                );
            }
            AstKind::Unary(op, e) => Ir::Unary(*op, Box::new(self.compile(e)?)),
            AstKind::Bin(op, a, b) => {
                Ir::Bin(*op, Box::new(self.compile(a)?), Box::new(self.compile(b)?))
            }
            AstKind::Logic(op, a, b) => {
                Ir::Logic(*op, Box::new(self.compile(a)?), Box::new(self.compile(b)?))
            }
            AstKind::Cond(c, a, b) => Ir::Cond(
                Box::new(self.compile(c)?),
                Box::new(self.compile(a)?),
                Box::new(self.compile(b)?),
            ),
        };
        Ok(Cv::Ir(ir))
    }

    fn member(&mut self, base: Cv, name: &str, span: Span) -> Result<Cv, ExprError> {
        match base {
            Cv::Path(src, prefix) => self.path_member(src, &prefix, name, span),
            Cv::Item(src) => {
                if name == "json" {
                    Ok(Cv::Path(src, String::new()))
                } else {
                    err(
                        format!("Propriété inconnue « {name} » : utilisez .json"),
                        span,
                    )
                }
            }
            Cv::Source(table) => match name {
                "item" => Ok(Cv::Item(RowSrc {
                    table,
                    row: RowSel::Current,
                })),
                "isExecuted" => Ok(Cv::Ir(Ir::Const(V::Bool(true)))),
                "first" | "last" | "all" => err(format!("Appelez la fonction : .{name}()"), span),
                _ => err(
                    format!(
                        "Propriété inconnue « {name} » (utilisez .first(), .last(), .all() ou .item)"
                    ),
                    span,
                ),
            },
            Cv::Ns(Ns::Math) => match name {
                "PI" => Ok(Cv::Ir(Ir::Const(V::Float(std::f64::consts::PI)))),
                "E" => Ok(Cv::Ir(Ir::Const(V::Float(std::f64::consts::E)))),
                _ => err(
                    format!("Math.{name} n'existe pas ou doit être appelé : Math.{name}(…)"),
                    span,
                ),
            },
            Cv::Ns(Ns::Node) => {
                let table = self.node_table(name, span)?;
                Ok(Cv::Item(RowSrc {
                    table,
                    row: RowSel::Current,
                }))
            }
            Cv::Ns(_) => err(
                format!("« {name} » doit être appelé comme une fonction"),
                span,
            ),
            Cv::Ir(ir) => Ok(Cv::Ir(Ir::Member(Box::new(ir), name.to_string()))),
        }
    }

    fn args(&mut self, args: &[Ast]) -> Result<Vec<Ir>, ExprError> {
        args.iter().map(|a| self.arg(a)).collect()
    }

    /// Argument : les fonctions fléchées ne sont admises qu'ici.
    fn arg(&mut self, a: &Ast) -> Result<Ir, ExprError> {
        if let AstKind::Arrow(params, body) = &a.kind {
            let base = self.locals.len();
            self.locals.extend(params.iter().cloned());
            let body = self.compile(body);
            self.locals.truncate(base);
            return Ok(Ir::Lambda(Arc::new(Lambda {
                base,
                params: params.len(),
                body: body?,
            })));
        }
        self.compile(a)
    }

    fn arity(name: &str, args: &[Ir], min: usize, max: usize, span: Span) -> Result<(), ExprError> {
        if args.len() < min || args.len() > max {
            let expected = if min == max {
                format!("{min}")
            } else if max == usize::MAX {
                format!("au moins {min}")
            } else {
                format!("{min} à {max}")
            };
            return err(
                format!(
                    "{name} attend {expected} argument(s), {} donné(s)",
                    args.len()
                ),
                span,
            );
        }
        Ok(())
    }

    fn call(&mut self, callee: &Ast, args: &[Ast], span: Span) -> Result<Cv, ExprError> {
        // Méthode : objet.nom(…)
        if let AstKind::Member(obj, name, name_span) = &callee.kind {
            let base = self.cv(obj)?;
            return self.method(base, name, args, *name_span, span);
        }
        let AstKind::Ident(name) = &callee.kind else {
            return err(
                "Seules les fonctions nommées peuvent être appelées",
                callee.span,
            );
        };
        if name == "$" {
            let [arg] = args else {
                return err("Écrivez $('Nom du nœud')", span);
            };
            let AstKind::Str(node) = &arg.kind else {
                return err(
                    "Le nom du nœud doit être un texte : $('Nom du nœud')",
                    arg.span,
                );
            };
            return Ok(Cv::Source(self.node_table(node, arg.span)?));
        }
        let args = self.args(args)?;
        let (func, min, max) = match name.as_str() {
            "$if" | "if" => (Func::If, 3, 3),
            "$ifEmpty" => (Func::IfEmpty, 2, 2),
            "$min" | "min" => (Func::Min, 1, usize::MAX),
            "$max" | "max" => (Func::Max, 1, usize::MAX),
            "Number" | "number" | "to_number" => (Func::Number, 1, 1),
            "String" | "string" | "to_string" => (Func::String, 1, 1),
            "Boolean" => (Func::Boolean, 1, 1),
            "parseInt" => (Func::ParseInt, 1, 2),
            "parseFloat" => (Func::ParseFloat, 1, 1),
            "upper" => (Func::Upper, 1, 1),
            "lower" => (Func::Lower, 1, 1),
            "trim" => (Func::Trim, 1, 1),
            "len" | "length" => (Func::Len, 1, 1),
            "concat" => (Func::Concat, 1, usize::MAX),
            "contains" => (Func::Contains, 2, 2),
            "starts_with" => (Func::StartsWith, 2, 2),
            "ends_with" => (Func::EndsWith, 2, 2),
            "replace" => (Func::ReplaceAll, 3, 3),
            "substr" => (Func::Substr, 2, 3),
            "round" => (Func::RoundTo, 1, 2),
            "floor" => (Func::Floor, 1, 1),
            "ceil" => (Func::Ceil, 1, 1),
            "abs" => (Func::Abs, 1, 1),
            "sqrt" => (Func::Sqrt, 1, 1),
            "pow" => (Func::Pow, 2, 2),
            "coalesce" => (Func::Coalesce, 1, usize::MAX),
            "is_null" => (Func::IsNull, 1, 1),
            _ => {
                let known: Vec<&str> = docs::DOCS
                    .iter()
                    .filter(|d| d.kind == "function")
                    .map(|d| d.name)
                    .collect();
                let mut msg = format!("Fonction inconnue : {name}()");
                if let Some(s) = suggest(name, known) {
                    msg.push_str(&format!(" — vouliez-vous dire {s}() ?"));
                }
                return err(msg, callee.span);
            }
        };
        Self::arity(&format!("{name}()"), &args, min, max, span)?;
        Ok(Cv::Ir(Ir::Func(func, args)))
    }

    fn method(
        &mut self,
        base: Cv,
        name: &str,
        args: &[Ast],
        name_span: Span,
        span: Span,
    ) -> Result<Cv, ExprError> {
        match base {
            Cv::Source(table) => {
                let batch_rows = self.batch(&table).map_or(0, RecordBatch::num_rows);
                match name {
                    "first" | "last" => {
                        if !args.is_empty() {
                            return err(format!(".{name}() ne prend pas d'argument"), span);
                        }
                        let row = if name == "first" {
                            0
                        } else {
                            batch_rows.saturating_sub(1)
                        };
                        let row = if batch_rows == 0 { usize::MAX } else { row };
                        Ok(Cv::Item(RowSrc {
                            table,
                            row: RowSel::Fixed(row),
                        }))
                    }
                    "all" => {
                        let Some(batch) = self.batch(&table) else {
                            return self.no_input(span);
                        };
                        if batch.num_rows() > MAX_ALL_ROWS {
                            return err(
                                format!(
                                    ".all() est limité à {MAX_ALL_ROWS} lignes ({} en entrée)",
                                    batch.num_rows()
                                ),
                                span,
                            );
                        }
                        let names = names_of(batch);
                        let rows = RowAccessor::new(batch)
                            .map_err(|e| ExprError::at(e.to_string(), span))?;
                        Ok(Cv::Ir(Ir::Const(V::list(
                            (0..batch.num_rows())
                                .map(|r| item(row_object(&rows, &names, r)))
                                .collect(),
                        ))))
                    }
                    _ => err(
                        format!(
                            "Méthode inconnue .{name}() (utilisez .first(), .last(), .all() ou .item)"
                        ),
                        name_span,
                    ),
                }
            }
            Cv::Ns(ns @ (Ns::Math | Ns::Json | Ns::Object)) => {
                let args = self.args(args)?;
                let (func, min, max) = match (ns, name) {
                    (Ns::Math, "round") => (Func::Round, 1, 1),
                    (Ns::Math, "floor") => (Func::Floor, 1, 1),
                    (Ns::Math, "ceil") => (Func::Ceil, 1, 1),
                    (Ns::Math, "trunc") => (Func::Trunc, 1, 1),
                    (Ns::Math, "abs") => (Func::Abs, 1, 1),
                    (Ns::Math, "sign") => (Func::Sign, 1, 1),
                    (Ns::Math, "sqrt") => (Func::Sqrt, 1, 1),
                    (Ns::Math, "pow") => (Func::Pow, 2, 2),
                    (Ns::Math, "exp") => (Func::Exp, 1, 1),
                    (Ns::Math, "log") => (Func::Log, 1, 1),
                    (Ns::Math, "log10") => (Func::Log10, 1, 1),
                    (Ns::Math, "min") => (Func::Min, 1, usize::MAX),
                    (Ns::Math, "max") => (Func::Max, 1, usize::MAX),
                    (Ns::Math, "random") => (Func::Random, 0, 0),
                    (Ns::Json, "stringify") => (Func::Stringify, 1, 3),
                    (Ns::Json, "parse") => (Func::Parse, 1, 1),
                    (Ns::Object, "keys") => (Func::Keys, 1, 1),
                    (Ns::Object, "values") => (Func::Values, 1, 1),
                    (Ns::Object, "entries") => (Func::Entries, 1, 1),
                    _ => {
                        let prefix = match ns {
                            Ns::Math => "Math",
                            Ns::Json => "JSON",
                            _ => "Object",
                        };
                        let known = docs::DOCS.iter().filter(|d| d.on == prefix).map(|d| d.name);
                        let mut msg = format!("{prefix}.{name}() n'existe pas");
                        if let Some(s) = suggest(name, known) {
                            msg.push_str(&format!(" — vouliez-vous dire {prefix}.{s}() ?"));
                        }
                        return err(msg, name_span);
                    }
                };
                Self::arity(&format!("{name}()"), &args, min, max, span)?;
                Ok(Cv::Ir(Ir::Func(func, args)))
            }
            other => {
                if docs::DOCS
                    .iter()
                    .any(|d| d.name == name && d.kind == "property")
                    && !docs::is_method(name)
                {
                    return err(
                        format!("{name} est une propriété : écrivez .{name} (sans parenthèses)"),
                        name_span,
                    );
                }
                if !docs::is_method(name) {
                    let candidates = docs::DOCS
                        .iter()
                        .filter(|d| {
                            d.kind == "method"
                                && matches!(
                                    d.on,
                                    "string" | "number" | "list" | "object" | "date" | "any"
                                )
                        })
                        .map(|d| d.name);
                    let mut msg = format!("Méthode inconnue .{name}()");
                    if let Some(s) = suggest(name, candidates) {
                        msg.push_str(&format!(" — vouliez-vous dire .{s}() ?"));
                    }
                    return err(msg, name_span);
                }
                let recv = self.value(other, span)?;
                let args = self.args(args)?;
                Ok(Cv::Ir(Ir::Method(Box::new(recv), name.to_string(), args)))
            }
        }
    }
}
