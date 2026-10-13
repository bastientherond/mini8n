//! Paramètres liés des requêtes de lecture : chaque expression `{{ … }}` de la requête
//! devient un marqueur du pilote (`$1`, `@mini8n_p1`, `:1`) et sa valeur est liée par le
//! pilote, jamais insérée dans le texte SQL (pas d'injection possible).

use mini8n_engine::arrow::array::RecordBatch;
use mini8n_engine::expr::{Expr, Scope, split_template};
use mini8n_engine::value::{RowAccessor, Value as Scalar};
use serde_json::Value;

/// Exécutions au plus en mode « pour chaque ligne ».
pub const MAX_PER_ROW: usize = 500;

enum Piece {
    Sql(String),
    /// Marqueur n° `usize` (à partir de 1).
    Param(usize),
}

/// Requête découpée en texte SQL et expressions à lier.
pub struct Bound {
    pieces: Vec<Piece>,
    exprs: Vec<Expr>,
}

impl Bound {
    pub fn parse(src: &str, scope: &Scope) -> Result<Self, String> {
        let mut pieces = Vec::new();
        let mut exprs = Vec::new();
        let mut skip_quote = false;
        for (is_expr, a, b) in split_template(src).map_err(|e| e.to_string())? {
            let text = &src[a..b];
            if !is_expr {
                let text = if skip_quote { &text[1..] } else { text };
                skip_quote = false;
                pieces.push(Piece::Sql(text.to_string()));
                continue;
            }
            // '{{ … }}' : guillemets superflus (la valeur est liée), ils sont retirés.
            if let Some(Piece::Sql(prev)) = pieces.last_mut()
                && prev.ends_with('\'')
                && src[b + 2..].starts_with('\'')
            {
                prev.pop();
                skip_quote = true;
            }
            let expr = Expr::compile(text, scope)
                .map_err(|e| format!("expression {{{{ {} }}}} : {}", text.trim(), e.message))?;
            exprs.push(expr);
            pieces.push(Piece::Param(exprs.len()));
        }
        Ok(Self { pieces, exprs })
    }

    /// Aucune expression : la requête est exécutée telle quelle.
    pub fn is_empty(&self) -> bool {
        self.exprs.is_empty()
    }

    /// Texte SQL avec le marqueur `marker(n)` à la place de chaque expression.
    pub fn sql(&self, marker: impl Fn(usize) -> String) -> String {
        self.pieces
            .iter()
            .map(|p| match p {
                Piece::Sql(s) => s.clone(),
                Piece::Param(n) => marker(*n),
            })
            .collect()
    }

    /// Valeurs à lier pour chaque exécution : une seule (première ligne de l'entrée
    /// éventuelle), ou une par ligne de l'entrée en mode `per_row`.
    pub fn values(
        &self,
        input: Option<&RecordBatch>,
        per_row: bool,
    ) -> Result<Vec<Vec<Value>>, String> {
        let accessor = match input {
            Some(b) if b.num_rows() > 0 => Some(RowAccessor::new(b).map_err(|e| e.to_string())?),
            _ => None,
        };
        let eval = |r: usize| -> Result<Vec<Value>, String> {
            self.exprs
                .iter()
                .map(|e| {
                    e.eval_value(accessor.as_ref(), r)
                        .map(json)
                        .map_err(|err| format!("ligne {} : {}", r + 1, err.message))
                })
                .collect()
        };
        match input {
            Some(b) if per_row => {
                if b.num_rows() > MAX_PER_ROW {
                    return Err(format!(
                        "{MAX_PER_ROW} exécutions au maximum ({} lignes en entrée)",
                        b.num_rows()
                    ));
                }
                (0..b.num_rows()).map(eval).collect()
            }
            _ => Ok(vec![eval(0)?]),
        }
    }
}

fn json(v: Scalar) -> Value {
    match v {
        Scalar::Null => Value::Null,
        Scalar::Bool(b) => Value::Bool(b),
        Scalar::Int(i) => Value::from(i),
        Scalar::Float(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        Scalar::Str(s) => Value::String(s),
    }
}

/// Valeur liée sous forme de texte (PostgreSQL : convertie ensuite dans le type attendu).
pub fn as_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mini8n_engine::data::json_to_batch;
    use serde_json::json;

    #[test]
    fn markers_and_values() {
        let input =
            json_to_batch(&json!([{"id": 7, "nom": "O'Brien"}, {"id": 8, "nom": "x"}])).unwrap();
        let scope = Scope::new(Some(&input));
        let b = Bound::parse(
            "SELECT * FROM t WHERE id = {{ $json.id }} AND nom = '{{ $json.nom }}' LIMIT {{ 1 + 1 }}",
            &scope,
        )
        .unwrap();
        assert_eq!(
            b.sql(|n| format!("${n}")),
            "SELECT * FROM t WHERE id = $1 AND nom = $2 LIMIT $3"
        );
        assert_eq!(
            b.values(Some(&input), false).unwrap(),
            [vec![json!(7), json!("O'Brien"), json!(2)]]
        );
        let all = b.values(Some(&input), true).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1][0], json!(8));

        let plain = Bound::parse("SELECT '{{' AS x", &Scope::new(None));
        assert!(plain.is_err(), "« {{{{ » sans « }}}} »");
        let none = Bound::parse("SELECT 1", &Scope::new(None)).unwrap();
        assert!(none.is_empty());
        assert_eq!(none.values(None, true).unwrap(), [Vec::<Value>::new()]);
    }
}
