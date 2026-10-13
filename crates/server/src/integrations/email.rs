//! Envoi d'e-mails (SMTP, lettre + rustls).

use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Mailboxes, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mini8n_engine::arrow::array::RecordBatch;
use mini8n_engine::data;
use serde_json::{Map, Value, json};

use super::template::{per_row, summary};
use super::{Ctx, str_param, table_of};

const MAX_PER_ROW: usize = 100;

async fn transport(
    ctx: &Ctx,
    p: &Map<String, Value>,
) -> Result<(AsyncSmtpTransport<Tokio1Executor>, Mailbox), String> {
    let c = ctx.credential(p, "smtp").await?;
    let port = str_param(&c, "port").parse::<u16>().unwrap_or(587);
    let policy = ctx.config.net_policy();
    // `localhost` éventuellement redirigé vers la machine qui héberge Docker.
    let host = policy.target_host(&str_param(&c, "host"));
    // Contrôle anti-SSRF de l'hôte (le nom est conservé pour la vérification TLS).
    policy.resolve(&host, port).await?;
    let builder = match str_param(&c, "security").as_str() {
        "TLS" => AsyncSmtpTransport::<Tokio1Executor>::relay(&host).map_err(|e| e.to_string())?,
        "aucune" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&host),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map_err(|e| e.to_string())?,
    };
    let mut builder = builder.port(port).timeout(Some(Duration::from_secs(30)));
    let user = str_param(&c, "user");
    if !user.is_empty() {
        builder = builder.credentials(Credentials::new(user, str_param(&c, "password")));
    }
    let from: Mailbox = str_param(&c, "from")
        .parse()
        .map_err(|_| "Adresse d'expéditeur invalide".to_string())?;
    Ok((builder.build(), from))
}

fn recipients(text: &str) -> Result<Mailboxes, String> {
    let list: Mailboxes = text
        .parse()
        .map_err(|_| format!("Destinataire invalide : « {text} »"))?;
    if list.iter().count() == 0 {
        return Err("Destinataire manquant".into());
    }
    if list.iter().count() > 50 {
        return Err("50 destinataires au maximum par e-mail".into());
    }
    Ok(list)
}

fn build(
    from: &Mailbox,
    to: Mailboxes,
    subject: &str,
    body: String,
    attachment: Option<(String, Vec<u8>, &str)>,
) -> Result<Message, String> {
    let mut b = Message::builder()
        .from(from.clone())
        .subject(subject.chars().take(250).collect::<String>());
    for m in to {
        b = b.to(m);
    }
    let text = SinglePart::builder()
        .header(ContentType::TEXT_PLAIN)
        .body(body);
    let msg = match attachment {
        None => b.singlepart(text),
        Some((name, bytes, mime)) => {
            let ct = ContentType::parse(mime).map_err(|e| e.to_string())?;
            b.multipart(
                MultiPart::mixed()
                    .singlepart(text)
                    .singlepart(Attachment::new(name).body(bytes, ct)),
            )
        }
    };
    msg.map_err(|e| format!("e-mail invalide : {e}"))
}

pub async fn send(
    ctx: &Ctx,
    p: &Map<String, Value>,
    input: RecordBatch,
) -> Result<RecordBatch, String> {
    let (to_t, subject_t, body_t) = (
        str_param(p, "to"),
        str_param(p, "subject"),
        str_param(p, "body"),
    );
    let fields = [
        ("Destinataire(s)", to_t.as_str()),
        ("Objet", subject_t.as_str()),
        ("Message", body_t.as_str()),
    ];
    let summary_mode = str_param(p, "mode") == "récapitulatif";
    if !summary_mode && input.num_rows() > MAX_PER_ROW {
        return Err(format!(
            "{MAX_PER_ROW} e-mails au maximum par exécution (utilisez le mode récapitulatif)"
        ));
    }
    // Textes rendus avant tout envoi : une erreur d'expression n'envoie rien.
    let texts = if summary_mode {
        vec![summary(&fields, &input)?]
    } else {
        per_row(&fields, &input)?
    };
    let (mailer, from) = transport(ctx, p).await?;
    let mut report = Vec::new();

    if summary_mode {
        let [to, subject, body] = <[String; 3]>::try_from(texts[0].clone())
            .map_err(|_| "textes incomplets".to_string())?;
        let attachment = match str_param(p, "attachment").as_str() {
            "csv" => Some((
                "donnees.csv".to_string(),
                data::batch_to_csv(&input)
                    .map_err(|e| e.to_string())?
                    .into_bytes(),
                "text/csv",
            )),
            "excel" => Some((
                "donnees.xlsx".to_string(),
                super::files::to_xlsx(&input)?,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            )),
            _ => None,
        };
        let msg = build(&from, recipients(&to)?, &subject, body, attachment)?;
        mailer
            .send(msg)
            .await
            .map_err(|e| format!("envoi impossible : {e}"))?;
        report.push(json!({ "destinataire": to, "objet": subject, "statut": "envoyé" }));
    } else {
        let mut sent = 0;
        for row in texts {
            let [to, subject, body] =
                <[String; 3]>::try_from(row).map_err(|_| "textes incomplets".to_string())?;
            let status = match recipients(&to).and_then(|r| build(&from, r, &subject, body, None)) {
                Ok(msg) => match mailer.send(msg).await {
                    Ok(_) => {
                        sent += 1;
                        "envoyé".to_string()
                    }
                    Err(e) => format!("erreur : {e}"),
                },
                Err(e) => format!("erreur : {e}"),
            };
            report.push(json!({ "destinataire": to, "objet": subject, "statut": status }));
        }
        if sent == 0 && input.num_rows() > 0 {
            return Err(format!(
                "Aucun e-mail envoyé ({})",
                report[0]["statut"].as_str().unwrap_or_default()
            ));
        }
    }
    table_of(Value::Array(report))
}

pub async fn test(ctx: &Ctx, p: &Map<String, Value>) -> Result<String, String> {
    let (mailer, _) = transport(ctx, p).await?;
    match mailer.test_connection().await {
        Ok(true) => Ok("Connexion au serveur SMTP réussie".into()),
        Ok(false) => Err("Le serveur SMTP ne répond pas".into()),
        Err(e) => Err(format!("connexion SMTP impossible : {e}")),
    }
}
