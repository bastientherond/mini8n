//! PostgreSQL et MySQL / MariaDB (sqlx).
//!
//! - Connexion à l'adresse IP validée par la politique réseau (pas de nouvelle résolution DNS).
//! - Lecture en protocole texte : tous les types de colonnes sont lisibles, puis convertis.
//! - Lecture avec expressions `{{ … }}` : valeurs liées par le pilote (voir `binds`).
//! - Écriture paramétrée ; les noms de table et de colonnes sont validés (pas d'injection SQL).

use std::time::Duration;

use futures_util::TryStreamExt;
use mini8n_engine::arrow::array::RecordBatch;
use serde_json::{Map, Value, json};
use sqlx::mysql::{MySqlConnectOptions, MySqlConnection, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Column, Connection, Row, TypeInfo};

use mini8n_engine::expr::Scope;

use super::binds::{Bound, as_text};
use super::{Ctx, num_param, rows_of, str_param, table_of};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ROWS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Db {
    Postgres,
    MySql,
}

impl Db {
    fn kind(self) -> &'static str {
        match self {
            Db::Postgres => "postgres",
            Db::MySql => "mysql",
        }
    }
}

enum Conn {
    Pg(PgConnection),
    My(MySqlConnection),
}

async fn connect(ctx: &Ctx, db: Db, params: &Map<String, Value>) -> Result<Conn, String> {
    let c = ctx.credential(params, db.kind()).await?;
    let host = str_param(&c, "host");
    let port = str_param(&c, "port")
        .parse::<u16>()
        .unwrap_or(if db == Db::Postgres { 5432 } else { 3306 });
    let addr = ctx.config.net_policy().resolve(&host, port).await?;
    let ip = addr.ip().to_string();
    let (user, password, database, ssl) = (
        str_param(&c, "user"),
        str_param(&c, "password"),
        str_param(&c, "database"),
        str_param(&c, "ssl"),
    );
    let fut = async {
        match db {
            Db::Postgres => {
                let opts = PgConnectOptions::new()
                    .host(&ip)
                    .port(addr.port())
                    .username(&user)
                    .password(&password)
                    .database(&database)
                    .application_name("mini8n")
                    .options([("statement_timeout", "120000")])
                    .ssl_mode(match ssl.as_str() {
                        "désactivé" => PgSslMode::Disable,
                        "requis" => PgSslMode::Require,
                        _ => PgSslMode::Prefer,
                    });
                PgConnection::connect_with(&opts).await.map(Conn::Pg)
            }
            Db::MySql => {
                let opts = MySqlConnectOptions::new()
                    .host(&ip)
                    .port(addr.port())
                    .username(&user)
                    .password(&password)
                    .database(&database)
                    .ssl_mode(match ssl.as_str() {
                        "désactivé" => MySqlSslMode::Disabled,
                        "requis" => MySqlSslMode::Required,
                        _ => MySqlSslMode::Preferred,
                    });
                MySqlConnection::connect_with(&opts).await.map(Conn::My)
            }
        }
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, fut).await {
        Ok(Ok(conn)) => Ok(conn),
        Ok(Err(e)) => Err(format!(
            "connexion à {host}:{port} impossible : {}",
            short(&e)
        )),
        Err(_) => Err(format!("connexion à {host}:{port} : délai dépassé")),
    }
}

/// Message d'erreur SQL sans détail de bas niveau.
fn short(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Database(d) => d.message().to_string(),
        other => other.to_string(),
    }
}

/// Valeur texte d'une colonne → JSON typé selon le type SQL.
fn convert(text: Option<String>, ty: &str) -> Value {
    let Some(t) = text else { return Value::Null };
    let ty = ty.to_ascii_uppercase();
    let base = ty.split(['(', ' ']).next().unwrap_or_default();
    match base {
        "INT2" | "INT4" | "INT8" | "SMALLINT" | "INTEGER" | "INT" | "BIGINT" | "TINYINT"
        | "MEDIUMINT" | "YEAR" | "SERIAL" | "BIGSERIAL" => t
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or(Value::String(t)),
        "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE" | "FLOAT" | "NUMERIC" | "DECIMAL" => t
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(Value::from)
            .unwrap_or(Value::String(t)),
        "BOOL" | "BOOLEAN" => Value::Bool(matches!(t.as_str(), "t" | "true" | "1")),
        "JSON" | "JSONB" => serde_json::from_str(&t).unwrap_or(Value::String(t)),
        _ => Value::String(t),
    }
}

fn row_json<R: Row>(row: &R) -> Map<String, Value>
where
    for<'r> Option<String>: sqlx::Decode<'r, R::Database>,
    for<'r> Option<Vec<u8>>: sqlx::Decode<'r, R::Database>,
    usize: sqlx::ColumnIndex<R>,
{
    let mut out = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        let ty = col.type_info().name().to_string();
        let text = match row.try_get_unchecked::<Option<String>, _>(i) {
            Ok(t) => t,
            Err(_) => row
                .try_get_unchecked::<Option<Vec<u8>>, _>(i)
                .ok()
                .flatten()
                .map(|b| String::from_utf8_lossy(&b).into_owned()),
        };
        let mut name = col.name().to_string();
        while out.contains_key(&name) {
            name.push_str("_2");
        }
        out.insert(name, convert(text, &ty));
    }
    out
}

pub async fn query(
    ctx: &Ctx,
    db: Db,
    p: &Map<String, Value>,
    input: Option<RecordBatch>,
) -> Result<RecordBatch, String> {
    let sql = str_param(p, "query");
    if sql.is_empty() {
        return Err("Requête SQL vide".into());
    }
    let max = (num_param(p, "max_rows", 10_000.0).max(1.0) as usize).min(MAX_ROWS);
    let bound = Bound::parse(&sql, &Scope::new(input.as_ref()))?;
    let sets = bound.values(input.as_ref(), str_param(p, "each") == "pour chaque ligne")?;
    if !bound.is_empty() && db == Db::Postgres && !is_select(&sql) {
        return Err(
            "Avec des expressions {{ … }}, la requête doit être un SELECT (ou WITH … SELECT)"
                .into(),
        );
    }
    let mut conn = connect(ctx, db, p).await?;
    let fut = async {
        let mut rows: Vec<Value> = Vec::new();
        match &mut conn {
            // Sans expression : protocole texte, la requête est envoyée telle quelle.
            _ if bound.is_empty() => {
                for _ in &sets {
                    fetch_text(&mut conn, &sql, max, &mut rows).await?;
                }
            }
            Conn::Pg(c) => {
                let sql = pg_prepare(c, &bound).await?;
                for values in &sets {
                    let mut q = sqlx::query(&sql);
                    for v in values {
                        q = q.bind(as_text(v));
                    }
                    let mut stream = q.fetch(&mut *c);
                    while let Some(row) = stream.try_next().await.map_err(|e| short(&e))? {
                        let text: String = row.try_get(0).map_err(|e| short(&e))?;
                        rows.push(serde_json::from_str(&text).map_err(|e| e.to_string())?);
                        if rows.len() >= max {
                            break;
                        }
                    }
                    if rows.len() >= max {
                        break;
                    }
                }
            }
            Conn::My(_) => {
                let sql = bound.sql(|n| format!("@mini8n_p{n}"));
                for values in &sets {
                    // Valeurs liées dans des variables de session, puis requête en protocole texte.
                    let Conn::My(c) = &mut conn else {
                        unreachable!()
                    };
                    for (i, v) in values.iter().enumerate() {
                        let set = format!("SET @mini8n_p{} = ?", i + 1);
                        let q = sqlx::query(&set);
                        let q = match v {
                            Value::Null => q.bind(Option::<String>::None),
                            Value::Bool(b) => q.bind(*b),
                            Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
                            Value::Number(n) => q.bind(n.as_f64()),
                            Value::String(s) => q.bind(s.clone()),
                            other => q.bind(other.to_string()),
                        };
                        q.execute(&mut *c).await.map_err(|e| short(&e))?;
                    }
                    fetch_text(&mut conn, &sql, max, &mut rows).await?;
                    if rows.len() >= max {
                        break;
                    }
                }
            }
        }
        Ok::<_, String>(rows)
    };
    let rows = tokio::time::timeout(QUERY_TIMEOUT, fut)
        .await
        .map_err(|_| "requête : délai dépassé".to_string())??;
    close(conn).await;
    table_of(Value::Array(rows))
}

/// Exécute `sql` en protocole texte et ajoute ses lignes (jusqu'à `max` au total).
async fn fetch_text(
    conn: &mut Conn,
    sql: &str,
    max: usize,
    rows: &mut Vec<Value>,
) -> Result<(), String> {
    if rows.len() >= max {
        return Ok(());
    }
    match conn {
        Conn::Pg(c) => {
            let mut stream = sqlx::raw_sql(sql).fetch(c);
            while let Some(row) = stream.try_next().await.map_err(|e| short(&e))? {
                rows.push(Value::Object(row_json(&row)));
                if rows.len() >= max {
                    break;
                }
            }
        }
        Conn::My(c) => {
            let mut stream = sqlx::raw_sql(sql).fetch(c);
            while let Some(row) = stream.try_next().await.map_err(|e| short(&e))? {
                rows.push(Value::Object(row_json(&row)));
                if rows.len() >= max {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn is_select(sql: &str) -> bool {
    let first = sql
        .trim_start_matches(|c: char| c.is_whitespace() || c == '(')
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(first.as_str(), "select" | "with" | "values" | "table")
}

/// Requête PostgreSQL à paramètres. Les valeurs sont liées en texte puis converties dans
/// le type que PostgreSQL déduit pour chaque `$n` (dates, nombres…). Les lignes reviennent
/// en JSON (`row_to_json`), car le protocole étendu renvoie des valeurs binaires.
async fn pg_prepare(c: &mut PgConnection, bound: &Bound) -> Result<String, String> {
    use sqlx::Either;
    use sqlx::Executor;

    let sql = bound.sql(|n| format!("${n}"));
    let sql = sql.trim().trim_end_matches(';');
    let described = c.describe(sql).await.map_err(|e| short(&e))?;
    let types: Vec<String> = match described.parameters() {
        Some(Either::Left(types)) => types.iter().map(|t| t.name().to_lowercase()).collect(),
        _ => vec![],
    };
    let typed = bound.sql(|n| match types.get(n - 1) {
        Some(t)
            if t != "text"
                && t != "unknown"
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_[] ".contains(c)) =>
        {
            format!("CAST(CAST(${n} AS text) AS {t})")
        }
        _ => format!("CAST(${n} AS text)"),
    });
    let typed = typed.trim().trim_end_matches(';');
    Ok(format!("SELECT row_to_json(t)::text FROM ({typed}) AS t"))
}

async fn close(conn: Conn) {
    let _ = match conn {
        Conn::Pg(c) => c.close().await,
        Conn::My(c) => c.close().await,
    };
}

pub async fn test(ctx: &Ctx, db: Db, p: &Map<String, Value>) -> Result<String, String> {
    let mut conn = connect(ctx, db, p).await?;
    let version: Result<String, sqlx::Error> = match &mut conn {
        Conn::Pg(c) => sqlx::query_scalar("SELECT version()").fetch_one(c).await,
        Conn::My(c) => sqlx::query_scalar("SELECT VERSION()").fetch_one(c).await,
    };
    close(conn).await;
    version
        .map(|v| {
            format!(
                "Connexion réussie : {}",
                v.chars().take(80).collect::<String>()
            )
        })
        .map_err(|e| short(&e))
}

/// `"{\"a\":1}"` → `{"a":1}` (les colonnes texte en reçoivent la représentation JSON).
fn embedded_json(v: Value) -> Value {
    match &v {
        Value::String(s) if s.starts_with('{') || s.starts_with('[') => {
            serde_json::from_str::<Value>(s)
                .ok()
                .filter(|p| p.is_object() || p.is_array())
                .unwrap_or(v)
        }
        _ => v,
    }
}

/// `schema.table` : identifiants simples uniquement.
fn valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && s.len() <= 63
}

pub fn valid_table(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    (1..=2).contains(&parts.len()) && parts.iter().all(|p| valid_ident(p))
}

fn columns(batch: &RecordBatch, lowercase: bool) -> Result<Vec<String>, String> {
    batch
        .schema()
        .fields()
        .iter()
        .map(|f| {
            let n = if lowercase {
                f.name().to_lowercase()
            } else {
                f.name().clone()
            };
            if valid_ident(&n) {
                Ok(n)
            } else {
                Err(format!(
                    "Colonne « {} » : nom non pris en charge (lettres, chiffres, _). Renommez-la.",
                    f.name()
                ))
            }
        })
        .collect()
}

pub async fn write(
    ctx: &Ctx,
    db: Db,
    p: &Map<String, Value>,
    input: RecordBatch,
) -> Result<RecordBatch, String> {
    let table = str_param(p, "table");
    if !valid_table(&table) {
        return Err("Nom de table invalide (format : table ou schema.table)".into());
    }
    let upsert = str_param(p, "mode") == "insertion ou mise à jour";
    let cols = columns(&input, db == Db::Postgres)?;
    if cols.is_empty() {
        return Err("La table d'entrée n'a aucune colonne".into());
    }
    let mut rows = rows_of(&input)?;
    if db == Db::Postgres {
        // Les identifiants non entre guillemets sont en minuscules dans PostgreSQL ;
        // un texte contenant un objet / tableau JSON est transmis comme tel (colonnes jsonb).
        rows = rows
            .into_iter()
            .map(|r| {
                r.into_iter()
                    .map(|(k, v)| (k.to_lowercase(), embedded_json(v)))
                    .collect()
            })
            .collect();
    }
    let total = rows.len();
    let mut conn = connect(ctx, db, p).await?;
    let fut = async {
        match &mut conn {
            Conn::Pg(c) => {
                let key: Vec<String> = str_param(p, "key")
                    .split(',')
                    .map(|k| k.trim().to_lowercase())
                    .filter(|k| !k.is_empty())
                    .collect();
                if upsert && (key.is_empty() || !key.iter().all(|k| valid_ident(k))) {
                    return Err("Clé de mise à jour invalide".to_string());
                }
                let list = cols.join(", ");
                let conflict = if upsert {
                    let updates: Vec<String> = cols
                        .iter()
                        .filter(|c| !key.contains(c))
                        .map(|c| format!("{c} = EXCLUDED.{c}"))
                        .collect();
                    if updates.is_empty() {
                        format!(" ON CONFLICT ({}) DO NOTHING", key.join(", "))
                    } else {
                        format!(
                            " ON CONFLICT ({}) DO UPDATE SET {}",
                            key.join(", "),
                            updates.join(", ")
                        )
                    }
                } else {
                    String::new()
                };
                let sql = format!(
                    "INSERT INTO {table} ({list}) SELECT {list} FROM jsonb_populate_recordset(NULL::{table}, $1){conflict}"
                );
                let mut tx = c.begin().await.map_err(|e| short(&e))?;
                for chunk in rows.chunks(1000) {
                    sqlx::query(&sql)
                        .bind(sqlx::types::Json(chunk))
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| short(&e))?;
                }
                tx.commit().await.map_err(|e| short(&e))?;
            }
            Conn::My(c) => {
                let per_chunk = (60_000 / cols.len()).clamp(1, 1000);
                let mut tx = c.begin().await.map_err(|e| short(&e))?;
                for chunk in rows.chunks(per_chunk) {
                    let placeholders = format!("({})", vec!["?"; cols.len()].join(", "));
                    let mut sql = format!(
                        "INSERT INTO {table} ({}) VALUES {}",
                        cols.join(", "),
                        vec![placeholders; chunk.len()].join(", ")
                    );
                    if upsert {
                        let updates: Vec<String> =
                            cols.iter().map(|c| format!("{c} = VALUES({c})")).collect();
                        sql.push_str(&format!(" ON DUPLICATE KEY UPDATE {}", updates.join(", ")));
                    }
                    let mut q = sqlx::query(&sql);
                    for row in chunk {
                        for col in &cols {
                            q = match row.get(col).unwrap_or(&Value::Null) {
                                Value::Null => q.bind(Option::<String>::None),
                                Value::Bool(b) => q.bind(*b),
                                Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
                                Value::Number(n) => q.bind(n.as_f64()),
                                Value::String(s) => q.bind(s.clone()),
                                other => q.bind(other.to_string()),
                            };
                        }
                    }
                    q.execute(&mut *tx).await.map_err(|e| short(&e))?;
                }
                tx.commit().await.map_err(|e| short(&e))?;
            }
        }
        Ok(())
    };
    let res = tokio::time::timeout(QUERY_TIMEOUT, fut)
        .await
        .map_err(|_| "écriture : délai dépassé".to_string());
    close(conn).await;
    res??;
    table_of(
        json!({ "table": table, "lignes": total, "operation": if upsert { "insertion ou mise à jour" } else { "insertion" } }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers() {
        assert!(valid_table("commandes"));
        assert!(valid_table("ventes.commandes_2026"));
        for bad in [
            "",
            "a.b.c",
            "1table",
            "t; DROP TABLE x",
            "t--",
            "\"t\"",
            "ta ble",
        ] {
            assert!(!valid_table(bad), "{bad}");
        }
    }

    #[test]
    fn conversions() {
        assert_eq!(convert(Some("42".into()), "INT4"), json!(42));
        assert_eq!(convert(Some("12.50".into()), "NUMERIC"), json!(12.5));
        assert_eq!(convert(Some("t".into()), "BOOL"), json!(true));
        assert_eq!(convert(Some("{\"a\":1}".into()), "JSONB"), json!({"a": 1}));
        assert_eq!(
            convert(Some("2026-10-12".into()), "DATE"),
            json!("2026-10-12")
        );
        assert_eq!(convert(None, "TEXT"), Value::Null);
        assert_eq!(convert(Some("10".into()), "DECIMAL(10,2)"), json!(10.0));
    }
}
