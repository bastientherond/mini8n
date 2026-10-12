//! Découpage en jetons (syntaxe JavaScript).

use super::{ExprError, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    /// Gabarit `` `texte ${expr}` `` : morceaux de texte et expressions (déjà découpées).
    Template(Vec<TplTok>),
    Ident(String),
    Punct(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TplTok {
    Text(String),
    Code(Vec<Token>, Span),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// Ponctuations, de la plus longue à la plus courte.
const PUNCTS: [&str; 34] = [
    "===", "!==", "...", "**", "?.", "??", "==", "!=", "<=", ">=", "&&", "||", "<>", "=>", "+",
    "-", "*", "/", "%", "<", ">", "!", "=", "(", ")", "[", "]", "{", "}", ",", ".", "?", ":", ";",
];

fn err<T>(msg: impl Into<String>, start: usize, end: usize) -> Result<T, ExprError> {
    Err(ExprError::at(msg, Span::new(start, end)))
}

pub fn tokenize(src: &str, base: usize) -> Result<Vec<Token>, ExprError> {
    let mut lx = Lexer {
        src,
        bytes: src.as_bytes(),
        pos: 0,
        base,
    };
    let mut out = Vec::new();
    while let Some(t) = lx.next_token()? {
        out.push(t);
    }
    Ok(out)
}

struct Lexer<'s> {
    src: &'s str,
    bytes: &'s [u8],
    pos: usize,
    /// Décalage de `src` dans le texte complet (positions des erreurs).
    base: usize,
}

impl Lexer<'_> {
    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        self.src.get(self.pos + offset..)?.chars().next()
    }

    fn next_token(&mut self) -> Result<Option<Token>, ExprError> {
        // Espaces et commentaires.
        loop {
            match self.peek_char() {
                Some(c) if c.is_whitespace() => self.pos += c.len_utf8(),
                Some('/') if self.char_at(1) == Some('/') => {
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += c.len_utf8();
                    }
                }
                Some('/') if self.char_at(1) == Some('*') => {
                    let start = self.pos;
                    match self.src[self.pos + 2..].find("*/") {
                        Some(end) => self.pos += end + 4,
                        None => {
                            return err(
                                "Commentaire non terminé",
                                self.base + start,
                                self.base + self.src.len(),
                            );
                        }
                    }
                }
                _ => break,
            }
        }
        let Some(c) = self.peek_char() else {
            return Ok(None);
        };
        let start = self.pos;
        let tok = if c.is_ascii_digit()
            || (c == '.' && self.char_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            self.number()?
        } else if c == '"' || c == '\'' {
            Tok::Str(self.string(c)?)
        } else if c == '`' {
            self.template()?
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while let Some(d) = self.peek_char() {
                if d.is_alphanumeric() || d == '_' || d == '$' {
                    self.pos += d.len_utf8();
                } else {
                    break;
                }
            }
            Tok::Ident(self.src[start..self.pos].to_string())
        } else {
            let rest = &self.src[self.pos..];
            let Some(p) = PUNCTS.iter().find(|p| rest.starts_with(**p)) else {
                return err(
                    format!("Caractère inattendu : « {c} »"),
                    self.base + start,
                    self.base + start + c.len_utf8(),
                );
            };
            // `a ?.5 : b` est un ternaire, pas un chaînage optionnel.
            if *p == "?." && self.char_at(2).is_some_and(|d| d.is_ascii_digit()) {
                self.pos += 1;
                Tok::Punct("?")
            } else {
                self.pos += p.len();
                Tok::Punct(p)
            }
        };
        Ok(Some(Token {
            tok,
            span: Span::new(self.base + start, self.base + self.pos),
        }))
    }

    fn number(&mut self) -> Result<Tok, ExprError> {
        let start = self.pos;
        let mut is_float = false;
        while self.pos < self.bytes.len() {
            let d = self.bytes[self.pos];
            if d.is_ascii_digit() || d == b'_' {
                self.pos += 1;
            } else if d == b'.'
                && !is_float
                && self.bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit)
            {
                is_float = true;
                self.pos += 1;
            } else if (d == b'e' || d == b'E')
                && self
                    .bytes
                    .get(self.pos + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == b'-' || *n == b'+')
            {
                is_float = true;
                self.pos += 2;
            } else {
                break;
            }
        }
        let text: String = self.src[start..self.pos]
            .chars()
            .filter(|c| *c != '_')
            .collect();
        let v = if is_float {
            text.parse::<f64>().ok().map(Tok::Float)
        } else {
            text.parse::<i64>()
                .map(Tok::Int)
                .or_else(|_| text.parse::<f64>().map(Tok::Float))
                .ok()
        };
        match v {
            Some(v) => Ok(v),
            None => err(
                format!("Nombre invalide : {text}"),
                self.base + start,
                self.base + self.pos,
            ),
        }
    }

    fn escape(&mut self, start: usize) -> Result<char, ExprError> {
        let Some(e) = self.peek_char() else {
            return err(
                "Chaîne non terminée",
                self.base + start,
                self.base + self.pos,
            );
        };
        self.pos += e.len_utf8();
        Ok(match e {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            'u' => {
                let hex = self.src.get(self.pos..self.pos + 4).unwrap_or("");
                let c = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                match c {
                    Some(c) => {
                        self.pos += 4;
                        c
                    }
                    None => {
                        return err(
                            "Séquence \\u invalide",
                            self.base + self.pos - 2,
                            self.base + self.pos,
                        );
                    }
                }
            }
            other => other,
        })
    }

    fn string(&mut self, quote: char) -> Result<String, ExprError> {
        let start = self.pos;
        self.pos += 1;
        let mut s = String::new();
        loop {
            let Some(d) = self.peek_char() else {
                return err(
                    "Chaîne non terminée",
                    self.base + start,
                    self.base + self.pos,
                );
            };
            self.pos += d.len_utf8();
            if d == quote {
                return Ok(s);
            }
            if d == '\\' {
                s.push(self.escape(start)?);
            } else {
                s.push(d);
            }
        }
    }

    fn template(&mut self) -> Result<Tok, ExprError> {
        let start = self.pos;
        self.pos += 1;
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            let Some(d) = self.peek_char() else {
                return err(
                    "Gabarit « ` » non terminé",
                    self.base + start,
                    self.base + self.pos,
                );
            };
            if d == '`' {
                self.pos += 1;
                break;
            }
            if d == '\\' {
                self.pos += 1;
                text.push(self.escape(start)?);
            } else if d == '$' && self.char_at(1) == Some('{') {
                if !text.is_empty() {
                    parts.push(TplTok::Text(std::mem::take(&mut text)));
                }
                self.pos += 2;
                let code_start = self.pos;
                let code_end = find_closing(self.src, code_start).ok_or_else(|| {
                    ExprError::at(
                        "« ${ » sans « } »",
                        Span::new(self.base + code_start - 2, self.base + self.src.len()),
                    )
                })?;
                let code = &self.src[code_start..code_end];
                let toks = tokenize(code, self.base + code_start)?;
                parts.push(TplTok::Code(
                    toks,
                    Span::new(self.base + code_start, self.base + code_end),
                ));
                self.pos = code_end + 1;
            } else {
                text.push(d);
                self.pos += d.len_utf8();
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(TplTok::Text(text));
        }
        Ok(Tok::Template(parts))
    }
}

/// Position de l'accolade fermante correspondante (en tenant compte des chaînes).
pub fn find_closing(src: &str, from: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in src[from..].char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' | '`' => quote = Some(c),
            '{' => depth += 1,
            '}' if depth == 0 => return Some(from + i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}
