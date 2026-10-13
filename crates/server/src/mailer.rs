//! E-mails de la plateforme (réinitialisation de mot de passe, vérification d'adresse,
//! invitations), envoyés par le SMTP configuré par l'administrateur (`SMTP_HOST`…).
//!
//! L'envoi se fait en tâche de fond : la réponse HTTP ne dépend ni du succès de l'envoi ni
//! de l'existence du compte (pas de déduction des adresses inscrites par le temps de réponse).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::MailConfig;

pub struct Mailer {
    transport: Transport,
    from: Mailbox,
}

enum Transport {
    Smtp(AsyncSmtpTransport<Tokio1Executor>),
    /// Tests : e-mails conservés en mémoire.
    Memory(Arc<Mutex<Vec<Sent>>>),
}

/// E-mail envoyé (boîte en mémoire des tests).
#[derive(Debug, Clone)]
pub struct Sent {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl Mailer {
    pub fn new(c: &MailConfig) -> Result<Self, String> {
        let builder = match c.security.as_str() {
            "tls" => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&c.host).map_err(|e| e.to_string())?
            }
            "aucune" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&c.host),
            _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&c.host)
                .map_err(|e| e.to_string())?,
        };
        let mut builder = builder.port(c.port).timeout(Some(Duration::from_secs(20)));
        if let Some(user) = &c.user {
            builder = builder.credentials(Credentials::new(
                user.clone(),
                c.password.clone().unwrap_or_default(),
            ));
        }
        Ok(Self {
            transport: Transport::Smtp(builder.build()),
            from: c
                .from
                .parse()
                .map_err(|_| "SMTP_FROM invalide".to_string())?,
        })
    }

    /// Boîte en mémoire (tests) : renvoie aussi la liste des e-mails envoyés.
    pub fn memory() -> (Self, Arc<Mutex<Vec<Sent>>>) {
        let outbox = Arc::new(Mutex::new(Vec::new()));
        let mailer = Self {
            transport: Transport::Memory(outbox.clone()),
            from: "mini8n <noreply@exemple.test>"
                .parse()
                .expect("adresse valide"),
        };
        (mailer, outbox)
    }

    /// Envoie un e-mail texte en tâche de fond (les erreurs sont journalisées).
    pub fn send_later(self: &Arc<Self>, to: &str, subject: String, body: String) {
        let mailer = self.clone();
        let to = to.to_string();
        tokio::spawn(async move {
            if let Err(e) = mailer.send(&to, &subject, body).await {
                tracing::error!(error = %e, "e-mail de la plateforme non envoyé");
            }
        });
    }

    async fn send(&self, to: &str, subject: &str, body: String) -> Result<(), String> {
        if let Transport::Memory(outbox) = &self.transport {
            outbox.lock().unwrap_or_else(|e| e.into_inner()).push(Sent {
                to: to.into(),
                subject: subject.into(),
                body,
            });
            return Ok(());
        }
        let to: Mailbox = to
            .parse()
            .map_err(|_| "destinataire invalide".to_string())?;
        let msg = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .map_err(|e| e.to_string())?;
        match &self.transport {
            Transport::Smtp(t) => t.send(msg).await.map(|_| ()).map_err(|e| e.to_string()),
            Transport::Memory(_) => Ok(()),
        }
    }
}
