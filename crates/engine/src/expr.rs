//! Petit langage d'expressions évalué ligne par ligne.
//!
//! ```text
//! prix * quantite > 100 && categorie == "livres"
//! upper(nom) + " (" + `code postal` + ")"
//! if(stock > 0, "dispo", "rupture")
//! ```
//!
//! Colonnes : identifiants (`prix`, `user.name`) ou entre backticks / crochets
//! (`` `code postal` ``, `[code postal]`).

use std::cmp::Ordering;

use arrow::datatypes::Schema;

use crate::error::{Result, fail};
use crate::value::{Num, RowAccessor, Value, values_cmp, values_eq};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(Value),
    Str(String),
    Ident(String),
    Quoted(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

const OPS: [&str; 18] = [
    "==", "!=", "<>", "<=", ">=", "&&", "||", "<", ">", "=", "+", "-", "*", "/", "%", "!", "and",
    "or",
];

fn tokenize(src: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            let mut is_float = false;
            while i < chars.len() {
                let d = chars[i];
                if d.is_ascii_digit() || d == '_' {
                    i += 1;
                } else if d == '.' && !is_float {
                    is_float = true;
                    i += 1;
                } else if (d == 'e' || d == 'E')
                    && chars
                        .get(i + 1)
                        .is_some_and(|n| n.is_ascii_digit() || *n == '-' || *n == '+')
                {
                    is_float = true;
                    i += 2;
                } else {
                    break;
                }
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            let v = if is_float {
                text.parse::<f64>().map(Value::Float).ok()
            } else {
                text.parse::<i64>()
                    .map(Value::Int)
                    .or_else(|_| text.parse::<f64>().map(Value::Float))
                    .ok()
            };
            match v {
                Some(v) => toks.push(Tok::Num(v)),
                None => return fail(format!("Nombre invalide : {text}")),
            }
        } else if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            let mut s = String::new();
            loop {
                let Some(&d) = chars.get(i) else {
                    return fail("Chaîne non terminée");
                };
                i += 1;
                if d == quote {
                    break;
                }
                if d == '\\' {
                    let Some(&e) = chars.get(i) else {
                        return fail("Chaîne non terminée");
                    };
                    i += 1;
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                } else {
                    s.push(d);
                }
            }
            toks.push(Tok::Str(s));
        } else if c == '`' || c == '[' {
            let close = if c == '`' { '`' } else { ']' };
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != close {
                i += 1;
            }
            if i >= chars.len() {
                return fail("Nom de colonne non terminé");
            }
            toks.push(Tok::Quoted(chars[start..i].iter().collect()));
            i += 1;
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.' | '$'))
            {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c == '(' {
            toks.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            toks.push(Tok::RParen);
            i += 1;
        } else if c == ',' {
            toks.push(Tok::Comma);
            i += 1;
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPS
                .iter()
                .filter(|op| op.len() <= 2 && !op.chars().all(char::is_alphabetic))
                .find(|op| rest.starts_with(**op));
            match op {
                Some(op) => {
                    toks.push(Tok::Op(op));
                    i += op.len();
                }
                None => return fail(format!("Caractère inattendu : '{c}'")),
            }
        }
    }
    Ok(toks)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Func {
    Upper,
    Lower,
    Trim,
    Len,
    Concat,
    Contains,
    StartsWith,
    EndsWith,
    Replace,
    Substr,
    Round,
    Floor,
    Ceil,
    Abs,
    Sqrt,
    Pow,
    Min,
    Max,
    Coalesce,
    If,
    IsNull,
    ToNumber,
    ToString,
}

impl Func {
    fn lookup(name: &str) -> Option<(Func, usize, usize)> {
        use Func::*;
        // (fonction, arité min, arité max)
        Some(match name.to_ascii_lowercase().as_str() {
            "upper" => (Upper, 1, 1),
            "lower" => (Lower, 1, 1),
            "trim" => (Trim, 1, 1),
            "len" | "length" => (Len, 1, 1),
            "concat" => (Concat, 1, usize::MAX),
            "contains" => (Contains, 2, 2),
            "starts_with" => (StartsWith, 2, 2),
            "ends_with" => (EndsWith, 2, 2),
            "replace" => (Replace, 3, 3),
            "substr" => (Substr, 2, 3),
            "round" => (Round, 1, 2),
            "floor" => (Floor, 1, 1),
            "ceil" => (Ceil, 1, 1),
            "abs" => (Abs, 1, 1),
            "sqrt" => (Sqrt, 1, 1),
            "pow" => (Pow, 2, 2),
            "min" => (Min, 2, usize::MAX),
            "max" => (Max, 2, usize::MAX),
            "coalesce" => (Coalesce, 1, usize::MAX),
            "if" => (If, 3, 3),
            "is_null" => (IsNull, 1, 1),
            "number" | "to_number" => (ToNumber, 1, 1),
            "string" | "to_string" => (ToString, 1, 1),
            _ => return None,
        })
    }
}

/// Liste des fonctions disponibles (affichée dans l'interface).
pub const FUNCTIONS: &[&str] = &[
    "upper(t)",
    "lower(t)",
    "trim(t)",
    "len(t)",
    "concat(a, b, …)",
    "contains(t, s)",
    "starts_with(t, s)",
    "ends_with(t, s)",
    "replace(t, a, b)",
    "substr(t, début, n?)",
    "round(x, décimales?)",
    "floor(x)",
    "ceil(x)",
    "abs(x)",
    "sqrt(x)",
    "pow(x, y)",
    "min(a, b, …)",
    "max(a, b, …)",
    "coalesce(a, b, …)",
    "if(cond, a, b)",
    "is_null(x)",
    "number(x)",
    "string(x)",
];

#[derive(Debug, Clone)]
enum Node {
    Lit(Value),
    Col(usize),
    Neg(Box<Node>),
    Not(Box<Node>),
    Bin(BinOp, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

/// Expression compilée pour un schéma donné.
#[derive(Debug, Clone)]
pub struct Expr {
    root: Node,
}

/// Garde-fous contre les expressions démesurées (pile du WebAssembly limitée).
const MAX_SOURCE_LEN: usize = 4_000;
const MAX_TOKENS: usize = 1_000;
const MAX_DEPTH: usize = 64;

struct Parser<'s> {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
    schema: &'s Schema,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn eat_op(&mut self, ops: &[&str]) -> Option<&'static str> {
        let found = match self.peek() {
            Some(Tok::Op(op)) if ops.contains(op) => Some(*op),
            Some(Tok::Ident(id)) => {
                let lower = id.to_ascii_lowercase();
                ops.iter()
                    .find(|o| o.chars().all(char::is_alphabetic) && **o == lower)
                    .and_then(|o| OPS.iter().find(|p| *p == o).copied())
            }
            _ => None,
        };
        if found.is_some() {
            self.pos += 1;
        }
        found
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Result<()> {
        if self.peek() == Some(&tok) {
            self.pos += 1;
            Ok(())
        } else {
            fail(format!("{what} attendu"))
        }
    }

    fn column(&self, name: &str) -> Result<Node> {
        match self.schema.index_of(name) {
            Ok(i) => Ok(Node::Col(i)),
            Err(_) => {
                let names: Vec<_> = self
                    .schema
                    .fields()
                    .iter()
                    .map(|f| f.name().as_str())
                    .collect();
                fail(format!(
                    "Colonne inconnue « {name} » (disponibles : {})",
                    if names.is_empty() {
                        "aucune".to_string()
                    } else {
                        names.join(", ")
                    }
                ))
            }
        }
    }

    fn parse_or(&mut self) -> Result<Node> {
        let mut left = self.parse_and()?;
        while self.eat_op(&["||", "or"]).is_some() {
            let right = self.parse_and()?;
            left = Node::Bin(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Node> {
        let mut left = self.parse_not()?;
        while self.eat_op(&["&&", "and"]).is_some() {
            let right = self.parse_not()?;
            left = Node::Bin(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Node> {
        if let Some(Tok::Ident(id)) = self.peek()
            && id.eq_ignore_ascii_case("not")
        {
            self.pos += 1;
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return fail("Expression trop imbriquée");
            }
            let inner = self.parse_not()?;
            self.depth -= 1;
            return Ok(Node::Not(Box::new(inner)));
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Node> {
        let left = self.parse_add()?;
        let op = match self.eat_op(&["==", "=", "!=", "<>", "<", "<=", ">", ">="]) {
            Some("==" | "=") => BinOp::Eq,
            Some("!=" | "<>") => BinOp::Ne,
            Some("<") => BinOp::Lt,
            Some("<=") => BinOp::Le,
            Some(">") => BinOp::Gt,
            Some(">=") => BinOp::Ge,
            _ => return Ok(left),
        };
        let right = self.parse_add()?;
        Ok(Node::Bin(op, Box::new(left), Box::new(right)))
    }

    fn parse_add(&mut self) -> Result<Node> {
        let mut left = self.parse_mul()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let right = self.parse_mul()?;
            let op = if op == "+" { BinOp::Add } else { BinOp::Sub };
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Node> {
        let mut left = self.parse_unary()?;
        while let Some(op) = self.eat_op(&["*", "/", "%"]) {
            let right = self.parse_unary()?;
            let op = match op {
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                _ => BinOp::Mod,
            };
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Node> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return fail("Expression trop imbriquée");
        }
        let node = self.parse_unary_inner();
        self.depth -= 1;
        node
    }

    fn parse_unary_inner(&mut self) -> Result<Node> {
        if self.eat_op(&["-"]).is_some() {
            return Ok(Node::Neg(Box::new(self.parse_unary()?)));
        }
        if self.eat_op(&["!"]).is_some() {
            return Ok(Node::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Node> {
        match self.next() {
            Some(Tok::Num(v)) => Ok(Node::Lit(v)),
            Some(Tok::Str(s)) => Ok(Node::Lit(Value::Str(s))),
            Some(Tok::Quoted(name)) => self.column(&name),
            Some(Tok::LParen) => {
                let e = self.parse_or()?;
                self.expect(Tok::RParen, "« ) »")?;
                Ok(e)
            }
            Some(Tok::Ident(id)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let Some((func, min, max)) = Func::lookup(&id) else {
                        return fail(format!("Fonction inconnue : {id}()"));
                    };
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.parse_or()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen, "« ) »")?;
                    if args.len() < min || args.len() > max {
                        return fail(format!("Nombre d'arguments invalide pour {id}()"));
                    }
                    return Ok(Node::Call(func, args));
                }
                // Une colonne existante est prioritaire sur les mots-clés.
                if let Ok(i) = self.schema.index_of(&id) {
                    return Ok(Node::Col(i));
                }
                match id.to_ascii_lowercase().as_str() {
                    "true" => Ok(Node::Lit(Value::Bool(true))),
                    "false" => Ok(Node::Lit(Value::Bool(false))),
                    "null" => Ok(Node::Lit(Value::Null)),
                    _ => self.column(&id),
                }
            }
            Some(t) => fail(format!("Élément inattendu : {t:?}")),
            None => fail("Expression incomplète"),
        }
    }
}

impl Expr {
    pub fn compile(src: &str, schema: &Schema) -> Result<Self> {
        if src.len() > MAX_SOURCE_LEN {
            return fail(format!(
                "Expression trop longue ({MAX_SOURCE_LEN} caractères max.)"
            ));
        }
        let toks = tokenize(src)?;
        if toks.is_empty() {
            return fail("Expression vide");
        }
        if toks.len() > MAX_TOKENS {
            return fail("Expression trop complexe");
        }
        let mut p = Parser {
            toks,
            pos: 0,
            depth: 0,
            schema,
        };
        let root = p.parse_or()?;
        if p.pos < p.toks.len() {
            return fail(format!("Élément inattendu : {:?}", p.toks[p.pos]));
        }
        Ok(Self { root })
    }

    pub fn eval(&self, row: &RowAccessor, r: usize) -> Result<Value> {
        eval(&self.root, row, r)
    }
}

fn num_or_fail(v: &Value, op: &str) -> Result<Option<Num>> {
    if v.is_null() {
        return Ok(None);
    }
    match v.as_num() {
        Some(n) => Ok(Some(n)),
        None => fail(format!(
            "Impossible d'appliquer « {op} » à {} « {} »",
            v.type_name(),
            v.to_text()
        )),
    }
}

fn arith(op: BinOp, a: Value, b: Value) -> Result<Value> {
    if op == BinOp::Add && (matches!(a, Value::Str(_)) || matches!(b, Value::Str(_))) {
        if a.is_null() || b.is_null() {
            return Ok(Value::Null);
        }
        return Ok(Value::Str(a.to_text() + &b.to_text()));
    }
    let sym = match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        _ => "%",
    };
    let (Some(x), Some(y)) = (num_or_fail(&a, sym)?, num_or_fail(&b, sym)?) else {
        return Ok(Value::Null);
    };
    if let (Num::I(x), Num::I(y)) = (x, y) {
        let r = match op {
            BinOp::Add => x.checked_add(y),
            BinOp::Sub => x.checked_sub(y),
            BinOp::Mul => x.checked_mul(y),
            BinOp::Mod if y == 0 => return Ok(Value::Null),
            BinOp::Mod => x.checked_rem(y),
            _ => None,
        };
        if let Some(r) = r {
            return Ok(Value::Int(r));
        }
    }
    let (x, y) = (x.f64(), y.f64());
    Ok(match op {
        BinOp::Add => Value::Float(x + y),
        BinOp::Sub => Value::Float(x - y),
        BinOp::Mul => Value::Float(x * y),
        BinOp::Div if y == 0.0 => Value::Null,
        BinOp::Div => Value::Float(x / y),
        BinOp::Mod if y == 0.0 => Value::Null,
        _ => Value::Float(x % y),
    })
}

fn text_arg(v: &Value) -> Option<String> {
    (!v.is_null()).then(|| v.to_text())
}

fn eval(node: &Node, row: &RowAccessor, r: usize) -> Result<Value> {
    Ok(match node {
        Node::Lit(v) => v.clone(),
        Node::Col(i) => row.get(*i, r),
        Node::Neg(e) => match eval(e, row, r)? {
            Value::Null => Value::Null,
            v => match num_or_fail(&v, "-")? {
                Some(Num::I(i)) => i
                    .checked_neg()
                    .map_or(Value::Float(-(i as f64)), Value::Int),
                Some(Num::F(f)) => Value::Float(-f),
                None => Value::Null,
            },
        },
        Node::Not(e) => Value::Bool(!eval(e, row, r)?.truthy()),
        Node::Bin(BinOp::And, a, b) => {
            Value::Bool(eval(a, row, r)?.truthy() && eval(b, row, r)?.truthy())
        }
        Node::Bin(BinOp::Or, a, b) => {
            Value::Bool(eval(a, row, r)?.truthy() || eval(b, row, r)?.truthy())
        }
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, row, r)?, eval(b, row, r)?);
            match op {
                BinOp::Eq => Value::Bool(values_eq(&a, &b)),
                BinOp::Ne => Value::Bool(!values_eq(&a, &b)),
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match values_cmp(&a, &b) {
                    None => Value::Null,
                    Some(o) => Value::Bool(match op {
                        BinOp::Lt => o == Ordering::Less,
                        BinOp::Le => o != Ordering::Greater,
                        BinOp::Gt => o == Ordering::Greater,
                        _ => o != Ordering::Less,
                    }),
                },
                _ => arith(*op, a, b)?,
            }
        }
        Node::Call(func, args) => {
            // `if` et `coalesce` sont paresseux.
            match func {
                Func::If => {
                    return if eval(&args[0], row, r)?.truthy() {
                        eval(&args[1], row, r)
                    } else {
                        eval(&args[2], row, r)
                    };
                }
                Func::Coalesce => {
                    for a in args {
                        let v = eval(a, row, r)?;
                        if !v.is_null() {
                            return Ok(v);
                        }
                    }
                    return Ok(Value::Null);
                }
                _ => {}
            }
            let vals = args
                .iter()
                .map(|a| eval(a, row, r))
                .collect::<Result<Vec<_>>>()?;
            call(*func, &vals)?
        }
    })
}

fn call(func: Func, a: &[Value]) -> Result<Value> {
    let text = |i: usize| text_arg(&a[i]);
    let num = |i: usize, name: &str| num_or_fail(&a[i], name);
    let map_f = |i: usize, name: &str, f: fn(f64) -> f64| -> Result<Value> {
        Ok(match num(i, name)? {
            None => Value::Null,
            Some(Num::I(x)) if matches!(name, "floor" | "ceil" | "round") => Value::Int(x),
            Some(n) => {
                let r = f(n.f64());
                if matches!(name, "floor" | "ceil") && r.abs() < 9.0e15 {
                    Value::Int(r as i64)
                } else {
                    Value::Float(r)
                }
            }
        })
    };
    Ok(match func {
        Func::Upper => text(0).map_or(Value::Null, |s| Value::Str(s.to_uppercase())),
        Func::Lower => text(0).map_or(Value::Null, |s| Value::Str(s.to_lowercase())),
        Func::Trim => text(0).map_or(Value::Null, |s| Value::Str(s.trim().to_string())),
        Func::Len => text(0).map_or(Value::Null, |s| Value::Int(s.chars().count() as i64)),
        Func::Concat => Value::Str(a.iter().map(Value::to_text).collect()),
        Func::Contains | Func::StartsWith | Func::EndsWith => match (text(0), text(1)) {
            (Some(s), Some(t)) => Value::Bool(match func {
                Func::Contains => s.contains(&t),
                Func::StartsWith => s.starts_with(&t),
                _ => s.ends_with(&t),
            }),
            _ => Value::Null,
        },
        Func::Replace => match (text(0), text(1), text(2)) {
            (Some(s), Some(from), Some(to)) => Value::Str(s.replace(&from, &to)),
            _ => Value::Null,
        },
        Func::Substr => {
            let Some(s) = text(0) else {
                return Ok(Value::Null);
            };
            let start = num(1, "substr")?.map_or(0, |n| n.f64().max(0.0) as usize);
            let count = if a.len() > 2 {
                num(2, "substr")?.map_or(usize::MAX, |n| n.f64().max(0.0) as usize)
            } else {
                usize::MAX
            };
            Value::Str(s.chars().skip(start).take(count).collect())
        }
        Func::Round => {
            let digits = if a.len() > 1 {
                num(1, "round")?.map_or(0, |n| n.f64() as i32)
            } else {
                0
            };
            if digits == 0 {
                map_f(0, "round", f64::round)?
            } else {
                match num(0, "round")? {
                    None => Value::Null,
                    Some(n) => {
                        let m = 10f64.powi(digits);
                        Value::Float((n.f64() * m).round() / m)
                    }
                }
            }
        }
        Func::Floor => map_f(0, "floor", f64::floor)?,
        Func::Ceil => map_f(0, "ceil", f64::ceil)?,
        Func::Sqrt => map_f(0, "sqrt", f64::sqrt)?,
        Func::Abs => match num(0, "abs")? {
            None => Value::Null,
            Some(Num::I(i)) => Value::Int(i.abs()),
            Some(Num::F(f)) => Value::Float(f.abs()),
        },
        Func::Pow => match (num(0, "pow")?, num(1, "pow")?) {
            (Some(x), Some(y)) => Value::Float(x.f64().powf(y.f64())),
            _ => Value::Null,
        },
        Func::Min | Func::Max => {
            let mut best: Option<&Value> = None;
            for v in a.iter().filter(|v| !v.is_null()) {
                best = match best {
                    None => Some(v),
                    Some(b) => {
                        let o = values_cmp(v, b).unwrap_or(Ordering::Equal);
                        let better = if func == Func::Min {
                            o == Ordering::Less
                        } else {
                            o == Ordering::Greater
                        };
                        Some(if better { v } else { b })
                    }
                };
            }
            best.cloned().unwrap_or(Value::Null)
        }
        Func::IsNull => Value::Bool(a[0].is_null()),
        Func::ToNumber => a[0].as_num().map_or(Value::Null, Num::value),
        Func::ToString => text(0).map_or(Value::Null, Value::Str),
        Func::If | Func::Coalesce => unreachable!("évaluées paresseusement"),
    })
}
