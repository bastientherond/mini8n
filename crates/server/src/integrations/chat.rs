//! Messages via webhooks entrants (Slack, Discord, Teams, Mattermost, Google Chat).
//! L'URL du webhook est un secret (identifiant chiffré) ; l'envoi passe par le client anti-SSRF.

use mini8n_engine::HttpRequest;
use mini8n_engine::arrow::array::RecordBatch;
use serde_json::{Map, Value, json};

use super::template::{render, summary_vars};
use super::{Ctx, rows_of, str_param, table_of};
use crate::net;

const MAX_PER_ROW: usize = 50;

/// Corps JSON attendu par chaque service.
pub fn payload(service: &str, text: &str) -> Value {
    match service {
        // Discord limite le contenu à 2000 caractères.
        "Discord" => json!({ "content": text.chars().take(2000).collect::<String>() }),
        _ => json!({ "text": text.chars().take(30_000).collect::<String>() }),
    }
}

async fn post(ctx: &Ctx, url: &str, service: &str, text: &str) -> Result<(), String> {
    let req = HttpRequest {
        method: "POST".into(),
        url: url.into(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: Some(payload(service, text).to_string()),
    };
    let res = net::fetch(req, &ctx.config.net_policy()).await?;
    if (200..300).contains(&res.status) {
        Ok(())
    } else {
        Err(format!(
            "HTTP {} : {}",
            res.status,
            res.body.chars().take(200).collect::<String>()
        ))
    }
}

pub async fn send(
    ctx: &Ctx,
    p: &Map<String, Value>,
    input: RecordBatch,
) -> Result<RecordBatch, String> {
    let c = ctx.credential(p, "chat_webhook").await?;
    let url = str_param(&c, "url");
    let service = str_param(p, "service");
    let template = str_param(p, "message");
    let rows = rows_of(&input)?;
    let mut report = Vec::new();
    if str_param(p, "mode") == "un par ligne" {
        if rows.len() > MAX_PER_ROW {
            return Err(format!(
                "{MAX_PER_ROW} messages au maximum par exécution (utilisez le mode récapitulatif)"
            ));
        }
        for row in &rows {
            let text = render(&template, row);
            post(ctx, &url, &service, &text).await?;
            report.push(json!({ "service": service, "message": text, "statut": "envoyé" }));
        }
    } else {
        let text = render(&template, &summary_vars(&rows));
        post(ctx, &url, &service, &text).await?;
        report.push(json!({ "service": service, "message": text, "statut": "envoyé" }));
    }
    table_of(Value::Array(report))
}

#[cfg(test)]
mod tests {
    use super::payload;
    use serde_json::json;

    #[test]
    fn payloads() {
        assert_eq!(payload("Slack", "hi"), json!({"text": "hi"}));
        assert_eq!(payload("Discord", "hi"), json!({"content": "hi"}));
        assert_eq!(
            payload("Discord", &"x".repeat(3000))["content"]
                .as_str()
                .unwrap()
                .len(),
            2000
        );
    }
}
