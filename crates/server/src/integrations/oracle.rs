//! Oracle Database, via le service interne « connectors » (pilote node-oracledb en mode
//! thin : open source, sans Oracle Instant Client). Le serveur résout et valide l'adresse
//! (politique réseau) ; le connecteur ne reçoit qu'une adresse IP.

use std::time::Duration;

use mini8n_engine::arrow::array::RecordBatch;
use serde_json::{Map, Value, json};

use super::sql::valid_table;
use super::{Ctx, num_param, rows_of, str_param, table_of};

async fn call(ctx: &Ctx, path: &str, body: Value) -> Result<Value, String> {
    let (Some(url), Some(token)) = (&ctx.config.connectors_url, &ctx.config.connectors_token)
    else {
        return Err("Connecteur Oracle non configuré (service « connectors »)".into());
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(150))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{url}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|_| "connecteur Oracle injoignable".to_string())?;
    let status = resp.status();
    let value: Value = resp
        .json()
        .await
        .map_err(|_| "réponse du connecteur illisible".to_string())?;
    if !status.is_success() {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("erreur du connecteur")
            .to_string());
    }
    Ok(value)
}

async fn connection(ctx: &Ctx, p: &Map<String, Value>) -> Result<Value, String> {
    let c = ctx.credential(p, "oracle").await?;
    let port = str_param(&c, "port").parse::<u16>().unwrap_or(1521);
    let addr = ctx
        .config
        .net_policy()
        .resolve(&str_param(&c, "host"), port)
        .await?;
    Ok(json!({
        "host": addr.ip().to_string(),
        "port": addr.port(),
        "service": str_param(&c, "service"),
        "user": str_param(&c, "user"),
        "password": str_param(&c, "password"),
    }))
}

pub async fn query(ctx: &Ctx, p: &Map<String, Value>) -> Result<RecordBatch, String> {
    let sql = str_param(p, "query");
    if sql.is_empty() {
        return Err("Requête SQL vide".into());
    }
    let max = num_param(p, "max_rows", 10_000.0).clamp(1.0, 100_000.0) as u64;
    let res = call(
        ctx,
        "/oracle/query",
        json!({ "connection": connection(ctx, p).await?, "sql": sql, "maxRows": max }),
    )
    .await?;
    let columns: Vec<String> = res["columns"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let rows: Vec<Value> = res["rows"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| {
                    let cells = r.as_array().cloned().unwrap_or_default();
                    Value::Object(columns.iter().cloned().zip(cells).collect())
                })
                .collect()
        })
        .unwrap_or_default();
    table_of(Value::Array(rows))
}

pub async fn write(
    ctx: &Ctx,
    p: &Map<String, Value>,
    input: RecordBatch,
) -> Result<RecordBatch, String> {
    let table = str_param(p, "table");
    if !valid_table(&table) {
        return Err("Nom de table invalide (format : TABLE ou SCHEMA.TABLE)".into());
    }
    let columns: Vec<String> = input
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    let rows: Vec<Value> = rows_of(&input)?
        .into_iter()
        .map(|r| {
            Value::Array(
                columns
                    .iter()
                    .map(|c| r.get(c).cloned().unwrap_or(Value::Null))
                    .collect(),
            )
        })
        .collect();
    let total = rows.len();
    call(
        ctx,
        "/oracle/write",
        json!({ "connection": connection(ctx, p).await?, "table": table, "columns": columns, "rows": rows }),
    )
    .await?;
    table_of(json!({ "table": table, "lignes": total, "operation": "insertion" }))
}

pub async fn test(ctx: &Ctx, p: &Map<String, Value>) -> Result<String, String> {
    let res = call(
        ctx,
        "/oracle/test",
        json!({ "connection": connection(ctx, p).await? }),
    )
    .await?;
    Ok(format!(
        "Connexion réussie : {}",
        res["version"].as_str().unwrap_or("Oracle")
    ))
}
