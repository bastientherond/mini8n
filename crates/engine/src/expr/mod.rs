//! Langage d'expressions : un sous-ensemble de JavaScript, comme dans n8n.
//!
//! ```text
//! $json.prix * $json.quantite > 100 && $json.categorie === "livres"
//! $json.nom.trim().toUpperCase() + " (" + ($json.age ?? "?") + ")"
//! $json.stock > 0 ? "dispo" : "rupture"
//! `Commande ${$json.id} du ${$now.format('dd/MM/yyyy')}`
//! $('Clients').first().json.email
//! $input.all().map(i => i.json.montant).sum()
//! ```
//!
//! Dans un champ texte, les expressions s'écrivent entre `{{ … }}` :
//! `Bonjour {{ $json.prenom }}, votre commande n°{{ $json.id }} est prête.`
//!
//! Les noms de colonnes nus (`prix > 15`) et les anciennes fonctions (`upper(nom)`)
//! restent acceptés.

mod compile;
pub mod docs;
mod eval;
mod lexer;
mod parser;
pub mod value;

use arrow::array::RecordBatch;
use serde::Serialize;

use crate::error::EngineError;
use crate::value::{RowAccessor, Value};
pub use compile::column_ref;
pub use value::V;

/// Garde-fous contre les expressions démesurées (pile du WebAssembly limitée).
const MAX_SOURCE_LEN: usize = 4_000;
const MAX_TEMPLATE_LEN: usize = 200_000;
const MAX_TOKENS: usize = 1_000;

/// Position dans le texte source (octets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExprError {
    pub message: String,
    pub span: Option<Span>,
}

impl ExprError {
    pub fn at(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span: Some(span),
        }
    }

    pub fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
        }
    }
}

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<ExprError> for EngineError {
    fn from(e: ExprError) -> Self {
        EngineError::Msg(e.message)
    }
}

/// Recherche de la sortie d'un autre nœud par son nom (`$('Nom')`).
pub type NodeLookup<'a> = &'a dyn Fn(&str) -> Option<RecordBatch>;

/// Contexte de compilation : données en entrée, autres nœuds, variables.
pub struct Scope<'a> {
    /// Table en entrée (`$json`, `$input`, noms de colonnes nus).
    pub input: Option<&'a RecordBatch>,
    /// Sorties des autres nœuds ; `None` si `$('…')` n'est pas disponible.
    pub nodes: Option<NodeLookup<'a>>,
    /// Noms des nœuds (suggestions).
    pub node_names: Vec<String>,
    /// Variables nommées supplémentaires (ex. `nb_lignes` des messages récapitulatifs).
    pub vars: Vec<(String, Value)>,
    pub now_ms: i64,
    /// Décalage horaire local en minutes (dates sans fuseau, mise en forme).
    pub offset_min: i32,
}

impl<'a> Scope<'a> {
    pub fn new(input: Option<&'a RecordBatch>) -> Self {
        Self {
            input,
            nodes: None,
            node_names: Vec::new(),
            vars: Vec::new(),
            now_ms: crate::host::now_ms() as i64,
            offset_min: crate::host::local_offset_min(),
        }
    }

    pub fn with_var(mut self, name: &str, value: Value) -> Self {
        self.vars.push((name.to_string(), value));
        self
    }
}

/// Expression compilée.
#[derive(Debug, Clone)]
pub struct Expr {
    ir: compile::Ir,
    names: Vec<String>,
    offset: i32,
}

fn compile_at(src: &str, base: usize, scope: &Scope) -> Result<Expr, ExprError> {
    if src.len() > MAX_SOURCE_LEN {
        return Err(ExprError::at(
            format!("Expression trop longue ({MAX_SOURCE_LEN} caractères max.)"),
            Span::new(base, base + src.len()),
        ));
    }
    let toks = lexer::tokenize(src, base)?;
    if toks.len() > MAX_TOKENS {
        return Err(ExprError::at(
            "Expression trop complexe",
            Span::new(base, base + src.len()),
        ));
    }
    let ast = parser::parse(toks, base + src.len())?;
    let ir = compile::Compiler::new(scope).compile(&ast)?;
    Ok(Expr {
        ir,
        names: scope
            .input
            .map(|b| {
                b.schema()
                    .fields()
                    .iter()
                    .map(|f| f.name().clone())
                    .collect()
            })
            .unwrap_or_default(),
        offset: scope.offset_min,
    })
}

impl Expr {
    /// Compile une expression ; un éventuel `{{ … }}` englobant est accepté.
    pub fn compile(src: &str, scope: &Scope) -> Result<Self, ExprError> {
        let trimmed = src.trim();
        if let Some(inner) = trimmed
            .strip_prefix("{{")
            .and_then(|s| s.strip_suffix("}}"))
            && !inner.contains("{{")
        {
            let base = src.find("{{").unwrap_or(0) + 2;
            return compile_at(inner, base, scope);
        }
        compile_at(src, 0, scope)
    }

    /// Évalue l'expression pour la ligne `r` de l'entrée.
    pub fn eval(&self, rows: Option<&RowAccessor>, r: usize) -> Result<V, ExprError> {
        let mut cx = eval::Cx {
            rows,
            names: &self.names,
            r,
            offset: self.offset,
            locals: Vec::new(),
            steps: 0,
        };
        cx.eval(&self.ir)
    }

    /// Valeur scalaire à écrire dans une table.
    pub fn eval_value(&self, rows: Option<&RowAccessor>, r: usize) -> Result<Value, ExprError> {
        Ok(self.eval(rows, r)?.into_value(self.offset))
    }

    pub fn offset(&self) -> i32 {
        self.offset
    }
}

#[derive(Debug, Clone)]
enum Part {
    Text(String),
    Expr(Expr),
}

/// Texte contenant des expressions `{{ … }}`.
#[derive(Debug, Clone)]
pub struct Template {
    parts: Vec<Part>,
    offset: i32,
}

/// Le texte contient-il des expressions `{{ … }}` ?
pub fn has_expressions(s: &str) -> bool {
    s.contains("{{")
}

/// Fin d'un bloc `{{ … }}` (position de `}}`), en ignorant les chaînes et les objets.
fn block_end(src: &str, from: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = from;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
        } else {
            match c {
                b'"' | b'\'' | b'`' => quote = Some(c),
                b'{' => depth += 1,
                b'}' if depth == 0 && bytes.get(i + 1) == Some(&b'}') => return Some(i),
                b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Découpe un texte en morceaux fixes et en expressions (positions dans `src`).
pub fn split_template(src: &str) -> Result<Vec<(bool, usize, usize)>, ExprError> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = src[pos..].find("{{") {
        let open = pos + rel;
        if open > pos {
            out.push((false, pos, open));
        }
        let Some(close) = block_end(src, open + 2) else {
            return Err(ExprError::at(
                "« {{ » sans « }} »",
                Span::new(open, src.len()),
            ));
        };
        out.push((true, open + 2, close));
        pos = close + 2;
    }
    if pos < src.len() {
        out.push((false, pos, src.len()));
    }
    Ok(out)
}

impl Template {
    pub fn compile(src: &str, scope: &Scope) -> Result<Self, ExprError> {
        if src.len() > MAX_TEMPLATE_LEN {
            return Err(ExprError::msg("Texte trop long"));
        }
        let mut parts = Vec::new();
        for (is_expr, start, end) in split_template(src)? {
            let piece = &src[start..end];
            if is_expr {
                if piece.trim().is_empty() {
                    return Err(ExprError::at(
                        "Expression vide : {{ }}",
                        Span::new(start - 2, end + 2),
                    ));
                }
                parts.push(Part::Expr(compile_at(piece, start, scope)?));
            } else {
                parts.push(Part::Text(piece.to_string()));
            }
        }
        Ok(Self {
            parts,
            offset: scope.offset_min,
        })
    }

    pub fn has_expressions(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Expr(_)))
    }

    /// Valeur : brute si le texte n'est qu'une expression, sinon le texte assemblé.
    pub fn eval(&self, rows: Option<&RowAccessor>, r: usize) -> Result<V, ExprError> {
        if let [Part::Expr(e)] = self.parts.as_slice() {
            return e.eval(rows, r);
        }
        self.render(rows, r).map(V::Str)
    }

    pub fn render(&self, rows: Option<&RowAccessor>, r: usize) -> Result<String, ExprError> {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                Part::Text(t) => out.push_str(t),
                Part::Expr(e) => out.push_str(&e.eval(rows, r)?.to_text(self.offset)),
            }
        }
        Ok(out)
    }
}

/// Rend un texte `{{ … }}` une seule fois (première ligne de l'entrée s'il y en a une).
pub fn render_once(src: &str, scope: &Scope) -> Result<String, ExprError> {
    if !has_expressions(src) {
        return Ok(src.to_string());
    }
    let t = Template::compile(src, scope)?;
    match scope.input {
        Some(batch) if batch.num_rows() > 0 => {
            let rows = RowAccessor::new(batch).map_err(|e| ExprError::msg(e.to_string()))?;
            t.render(Some(&rows), 0)
        }
        _ => t.render(None, 0),
    }
}

/// Résultat d'un essai d'expression dans l'éditeur.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trial {
    pub ok: bool,
    /// Valeur (JSON) ; pour un texte à expressions : le texte assemblé.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    /// Type de la valeur : string, number, boolean, list, object, date, null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Position de l'erreur dans le texte (octets).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// Nombre de lignes en entrée.
    pub rows: usize,
}

/// Évalue une expression (`template = false`) ou un texte `{{ … }}` sur la ligne `row`.
pub fn trial(src: &str, template: bool, scope: &Scope, row: usize) -> Trial {
    let rows_count = scope.input.map_or(0, RecordBatch::num_rows);
    let fail = |e: ExprError| Trial {
        ok: false,
        value: None,
        kind: None,
        error: Some(e.message),
        span: e.span,
        rows: rows_count,
    };
    let accessor = match scope.input.map(RowAccessor::new).transpose() {
        Ok(a) => a,
        Err(e) => return fail(ExprError::msg(e.to_string())),
    };
    let r = row.min(rows_count.saturating_sub(1));
    let rows = accessor.as_ref().filter(|_| rows_count > 0);
    let result = if template {
        Template::compile(src, scope).and_then(|t| {
            if t.has_expressions() {
                t.eval(rows, r)
            } else {
                Ok(V::Str(src.to_string()))
            }
        })
    } else {
        Expr::compile(src, scope).and_then(|e| e.eval(rows, r))
    };
    match result {
        Ok(v) => Trial {
            ok: true,
            kind: Some(v.kind()),
            value: Some(v.to_json(scope.offset_min)),
            error: None,
            span: None,
            rows: rows_count,
        },
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests;
