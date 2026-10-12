//! Analyse syntaxique : jetons → arbre (sous-ensemble d'expressions JavaScript).

use super::lexer::{Tok, Token, TplTok};
use super::{ExprError, Span};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    /// `==` (comparaison souple, comme en JavaScript).
    Eq,
    Ne,
    /// `===` (types identiques).
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Logic {
    And,
    Or,
    Nullish,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
}

#[derive(Debug, Clone)]
pub enum TplPart {
    Text(String),
    Expr(Ast),
}

#[derive(Debug, Clone)]
pub enum AstKind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Template(Vec<TplPart>),
    Ident(String),
    Array(Vec<Ast>),
    Object(Vec<(String, Ast)>),
    Member(Box<Ast>, String, Span),
    Index(Box<Ast>, Box<Ast>),
    Call(Box<Ast>, Vec<Ast>),
    Arrow(Vec<String>, Box<Ast>),
    Unary(UnOp, Box<Ast>),
    Bin(BinOp, Box<Ast>, Box<Ast>),
    Logic(Logic, Box<Ast>, Box<Ast>),
    Cond(Box<Ast>, Box<Ast>, Box<Ast>),
}

#[derive(Debug, Clone)]
pub struct Ast {
    pub kind: AstKind,
    pub span: Span,
}

const MAX_DEPTH: usize = 64;

pub fn parse(toks: Vec<Token>, end: usize) -> Result<Ast, ExprError> {
    let mut p = Parser {
        toks,
        pos: 0,
        depth: 0,
        end,
    };
    if p.toks.is_empty() {
        return Err(ExprError::at("Expression vide", Span::new(end, end)));
    }
    let ast = p.expression()?;
    // Un « ; » final est toléré.
    if p.is_punct(";") {
        p.pos += 1;
    }
    if let Some(t) = p.toks.get(p.pos) {
        return Err(ExprError::at(
            format!("Élément inattendu : {}", describe(&t.tok)),
            t.span,
        ));
    }
    Ok(ast)
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Int(i) => i.to_string(),
        Tok::Float(f) => f.to_string(),
        Tok::Str(s) => format!("\"{s}\""),
        Tok::Template(_) => "gabarit `…`".into(),
        Tok::Ident(s) => format!("« {s} »"),
        Tok::Punct(p) => format!("« {p} »"),
    }
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(id)) if id.eq_ignore_ascii_case(w))
    }

    fn span_here(&self) -> Span {
        self.toks
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::new(self.end, self.end))
    }

    fn prev_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.toks.get(i))
            .map_or(self.end, |t| t.span.end)
    }

    fn expect(&mut self, p: &str) -> Result<(), ExprError> {
        if self.is_punct(p) {
            self.pos += 1;
            Ok(())
        } else {
            let found = self
                .peek()
                .map_or_else(|| "la fin de l'expression".to_string(), describe);
            Err(ExprError::at(
                format!("« {p} » attendu (trouvé : {found})"),
                self.span_here(),
            ))
        }
    }

    fn enter(&mut self) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ExprError::at("Expression trop imbriquée", self.span_here()));
        }
        Ok(())
    }

    fn node(&self, kind: AstKind, start: usize) -> Ast {
        Ast {
            kind,
            span: Span::new(start, self.prev_end().max(start)),
        }
    }

    pub fn expression(&mut self) -> Result<Ast, ExprError> {
        self.enter()?;
        let r = self.conditional();
        self.depth -= 1;
        r
    }

    fn conditional(&mut self) -> Result<Ast, ExprError> {
        let start = self.span_here().start;
        if let Some(arrow) = self.try_arrow()? {
            return Ok(arrow);
        }
        let cond = self.logic_or()?;
        if self.is_punct("?") {
            self.pos += 1;
            let a = self.expression()?;
            self.expect(":")?;
            let b = self.expression()?;
            return Ok(self.node(
                AstKind::Cond(Box::new(cond), Box::new(a), Box::new(b)),
                start,
            ));
        }
        Ok(cond)
    }

    /// `x => …` ou `(a, b) => …`.
    fn try_arrow(&mut self) -> Result<Option<Ast>, ExprError> {
        let start = self.span_here().start;
        let save = self.pos;
        let mut params = Vec::new();
        match self.peek() {
            Some(Tok::Ident(id))
                if matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.tok),
                    Some(Tok::Punct("=>"))
                ) =>
            {
                params.push(id.clone());
                self.pos += 1;
            }
            Some(Tok::Punct("(")) => {
                self.pos += 1;
                loop {
                    match self.peek() {
                        Some(Tok::Punct(")")) => {
                            self.pos += 1;
                            break;
                        }
                        Some(Tok::Ident(id)) => {
                            params.push(id.clone());
                            self.pos += 1;
                            if self.is_punct(",") {
                                self.pos += 1;
                            } else if !self.is_punct(")") {
                                self.pos = save;
                                return Ok(None);
                            }
                        }
                        _ => {
                            self.pos = save;
                            return Ok(None);
                        }
                    }
                }
                if !self.is_punct("=>") {
                    self.pos = save;
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }
        self.expect("=>")?;
        let body = self.expression()?;
        Ok(Some(
            self.node(AstKind::Arrow(params, Box::new(body)), start),
        ))
    }

    fn logic_or(&mut self) -> Result<Ast, ExprError> {
        let start = self.span_here().start;
        let mut left = self.logic_and()?;
        loop {
            let op = if self.is_punct("||") || self.is_word("or") {
                Logic::Or
            } else if self.is_punct("??") {
                Logic::Nullish
            } else {
                break;
            };
            self.pos += 1;
            let right = self.logic_and()?;
            left = self.node(AstKind::Logic(op, Box::new(left), Box::new(right)), start);
        }
        Ok(left)
    }

    fn logic_and(&mut self) -> Result<Ast, ExprError> {
        let start = self.span_here().start;
        let mut left = self.equality()?;
        while self.is_punct("&&") || self.is_word("and") {
            self.pos += 1;
            let right = self.equality()?;
            left = self.node(
                AstKind::Logic(Logic::And, Box::new(left), Box::new(right)),
                start,
            );
        }
        Ok(left)
    }

    fn binary_op(&self, table: &[(&str, BinOp)]) -> Option<BinOp> {
        match self.peek() {
            Some(Tok::Punct(p)) => table.iter().find(|(s, _)| s == p).map(|(_, op)| *op),
            _ => None,
        }
    }

    fn equality(&mut self) -> Result<Ast, ExprError> {
        const OPS: [(&str, BinOp); 6] = [
            ("===", BinOp::StrictEq),
            ("!==", BinOp::StrictNe),
            ("==", BinOp::Eq),
            ("=", BinOp::Eq),
            ("!=", BinOp::Ne),
            ("<>", BinOp::Ne),
        ];
        let start = self.span_here().start;
        let mut left = self.relational()?;
        while let Some(op) = self.binary_op(&OPS) {
            self.pos += 1;
            let right = self.relational()?;
            left = self.node(AstKind::Bin(op, Box::new(left), Box::new(right)), start);
        }
        Ok(left)
    }

    fn relational(&mut self) -> Result<Ast, ExprError> {
        const OPS: [(&str, BinOp); 4] = [
            ("<", BinOp::Lt),
            ("<=", BinOp::Le),
            (">", BinOp::Gt),
            (">=", BinOp::Ge),
        ];
        let start = self.span_here().start;
        let mut left = self.additive()?;
        while let Some(op) = self.binary_op(&OPS) {
            self.pos += 1;
            let right = self.additive()?;
            left = self.node(AstKind::Bin(op, Box::new(left), Box::new(right)), start);
        }
        Ok(left)
    }

    fn additive(&mut self) -> Result<Ast, ExprError> {
        const OPS: [(&str, BinOp); 2] = [("+", BinOp::Add), ("-", BinOp::Sub)];
        let start = self.span_here().start;
        let mut left = self.multiplicative()?;
        while let Some(op) = self.binary_op(&OPS) {
            self.pos += 1;
            let right = self.multiplicative()?;
            left = self.node(AstKind::Bin(op, Box::new(left), Box::new(right)), start);
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Result<Ast, ExprError> {
        const OPS: [(&str, BinOp); 3] = [("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Mod)];
        let start = self.span_here().start;
        let mut left = self.exponent()?;
        while let Some(op) = self.binary_op(&OPS) {
            self.pos += 1;
            let right = self.exponent()?;
            left = self.node(AstKind::Bin(op, Box::new(left), Box::new(right)), start);
        }
        Ok(left)
    }

    fn exponent(&mut self) -> Result<Ast, ExprError> {
        let start = self.span_here().start;
        let base = self.unary()?;
        if self.is_punct("**") {
            self.pos += 1;
            self.enter()?;
            let exp = self.exponent()?;
            self.depth -= 1;
            return Ok(self.node(
                AstKind::Bin(BinOp::Pow, Box::new(base), Box::new(exp)),
                start,
            ));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<Ast, ExprError> {
        let start = self.span_here().start;
        let op = if self.is_punct("-") {
            Some(UnOp::Neg)
        } else if self.is_punct("+") {
            Some(UnOp::Plus)
        } else if self.is_punct("!") || self.is_word("not") {
            Some(UnOp::Not)
        } else {
            None
        };
        if let Some(op) = op {
            self.pos += 1;
            self.enter()?;
            // `not` porte sur toute la comparaison qui suit (ancienne syntaxe).
            let inner =
                if op == UnOp::Not && self.toks[self.pos - 1].tok == Tok::Ident("not".into()) {
                    self.equality()?
                } else {
                    self.unary()?
                };
            self.depth -= 1;
            return Ok(self.node(AstKind::Unary(op, Box::new(inner)), start));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Ast, ExprError> {
        let start = self.span_here().start;
        let mut e = self.primary()?;
        loop {
            if self.is_punct(".") || self.is_punct("?.") {
                self.pos += 1;
                // `a?.[0]` et `f?.()`.
                if self.is_punct("[") || self.is_punct("(") {
                    continue;
                }
                let span = self.span_here();
                match self.peek() {
                    Some(Tok::Ident(name)) => {
                        let name = name.clone();
                        self.pos += 1;
                        e = self.node(AstKind::Member(Box::new(e), name, span), start);
                    }
                    _ => {
                        return Err(ExprError::at("Nom de propriété attendu après « . »", span));
                    }
                }
            } else if self.is_punct("[") {
                self.pos += 1;
                let index = self.expression()?;
                self.expect("]")?;
                e = self.node(AstKind::Index(Box::new(e), Box::new(index)), start);
            } else if self.is_punct("(") {
                self.pos += 1;
                let args = self.list(")")?;
                e = self.node(AstKind::Call(Box::new(e), args), start);
            } else {
                return Ok(e);
            }
        }
    }

    fn list(&mut self, close: &str) -> Result<Vec<Ast>, ExprError> {
        let mut items = Vec::new();
        loop {
            if self.is_punct(close) {
                self.pos += 1;
                return Ok(items);
            }
            items.push(self.expression()?);
            if self.is_punct(",") {
                self.pos += 1;
            } else {
                self.expect(close)?;
                return Ok(items);
            }
        }
    }

    fn primary(&mut self) -> Result<Ast, ExprError> {
        let span = self.span_here();
        let Some(tok) = self.toks.get(self.pos).map(|t| t.tok.clone()) else {
            return Err(ExprError::at("Expression incomplète", span));
        };
        self.pos += 1;
        let kind = match tok {
            Tok::Int(i) => AstKind::Int(i),
            Tok::Float(f) => AstKind::Float(f),
            Tok::Str(s) => AstKind::Str(s),
            Tok::Template(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    out.push(match part {
                        TplTok::Text(t) => TplPart::Text(t),
                        TplTok::Code(toks, span) => {
                            self.enter()?;
                            let ast = parse(toks, span.end)?;
                            self.depth -= 1;
                            TplPart::Expr(ast)
                        }
                    });
                }
                AstKind::Template(out)
            }
            Tok::Ident(id) => match id.as_str() {
                "true" => AstKind::Bool(true),
                "false" => AstKind::Bool(false),
                "null" | "undefined" => AstKind::Null,
                _ => AstKind::Ident(id),
            },
            Tok::Punct("(") => {
                let e = self.expression()?;
                self.expect(")")?;
                return Ok(Ast {
                    kind: e.kind,
                    span: Span::new(span.start, self.prev_end()),
                });
            }
            Tok::Punct("[") => {
                self.enter()?;
                let items = self.list("]")?;
                self.depth -= 1;
                AstKind::Array(items)
            }
            Tok::Punct("{") => {
                self.enter()?;
                let mut fields = Vec::new();
                loop {
                    if self.is_punct("}") {
                        self.pos += 1;
                        break;
                    }
                    let key_span = self.span_here();
                    let key = match self.peek().cloned() {
                        Some(Tok::Ident(k)) | Some(Tok::Str(k)) => k,
                        Some(Tok::Int(i)) => i.to_string(),
                        _ => return Err(ExprError::at("Nom de champ attendu", key_span)),
                    };
                    self.pos += 1;
                    let value = if self.is_punct(":") {
                        self.pos += 1;
                        self.expression()?
                    } else {
                        // Raccourci `{ nom }`.
                        Ast {
                            kind: AstKind::Ident(key.clone()),
                            span: key_span,
                        }
                    };
                    fields.push((key, value));
                    if self.is_punct(",") {
                        self.pos += 1;
                    } else {
                        self.expect("}")?;
                        break;
                    }
                }
                self.depth -= 1;
                AstKind::Object(fields)
            }
            other => {
                return Err(ExprError::at(
                    format!("Élément inattendu : {}", describe(&other)),
                    span,
                ));
            }
        };
        Ok(self.node(kind, span.start))
    }
}
