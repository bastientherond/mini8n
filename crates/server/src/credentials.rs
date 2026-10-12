//! Identifiants des intégrations, chiffrés au repos.
//!
//! - Les champs secrets (mots de passe, URL de webhook) ne sont **jamais** renvoyés au navigateur :
//!   l'API n'expose qu'un résumé des champs non secrets.
//! - À la modification, un champ secret laissé vide conserve sa valeur.
//! - Les workflows ne contiennent que l'identifiant de l'identifiant : un export de workflow
//!   ne divulgue aucun secret.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use sqlx::types::Json as SqlJson;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::crypto::Vault;
use crate::error::{AppError, AppResult};
use crate::validate;
use crate::workspaces::require_member;

const MAX_CREDENTIALS: i64 = 200;

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub name: &'static str,
    pub label: &'static str,
    /// text | number | password | select
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub secret: bool,
    pub required: bool,
    pub default: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<&'static str>,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub placeholder: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Kind {
    pub kind: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub fields: Vec<Field>,
}

fn field(
    name: &'static str,
    label: &'static str,
    ty: &'static str,
    required: bool,
    default: Value,
) -> Field {
    Field {
        name,
        label,
        ty,
        secret: ty == "password",
        required,
        default,
        options: vec![],
        placeholder: "",
    }
}

impl Field {
    fn options(mut self, o: &[&'static str]) -> Self {
        self.options = o.to_vec();
        self
    }
    fn placeholder(mut self, p: &'static str) -> Self {
        self.placeholder = p;
        self
    }
}

pub fn kinds() -> Vec<Kind> {
    let ssl = |default: &'static str| {
        field("ssl", "Chiffrement (TLS)", "select", true, json!(default)).options(&[
            "désactivé",
            "préféré",
            "requis",
        ])
    };
    vec![
        Kind {
            kind: "postgres",
            label: "PostgreSQL",
            description: "Base PostgreSQL (lecture et écriture).",
            fields: vec![
                field("host", "Hôte", "text", true, json!("")).placeholder("db.exemple.com"),
                field("port", "Port", "number", true, json!(5432)),
                field("database", "Base", "text", true, json!("")),
                field("user", "Utilisateur", "text", true, json!("")),
                field("password", "Mot de passe", "password", false, json!("")),
                ssl("préféré"),
            ],
        },
        Kind {
            kind: "mysql",
            label: "MySQL / MariaDB",
            description: "Base MySQL ou MariaDB (lecture et écriture).",
            fields: vec![
                field("host", "Hôte", "text", true, json!("")).placeholder("db.exemple.com"),
                field("port", "Port", "number", true, json!(3306)),
                field("database", "Base", "text", true, json!("")),
                field("user", "Utilisateur", "text", true, json!("")),
                field("password", "Mot de passe", "password", false, json!("")),
                ssl("préféré"),
            ],
        },
        Kind {
            kind: "oracle",
            label: "Oracle Database",
            description: "Base Oracle (pilote « thin », sans client Oracle à installer).",
            fields: vec![
                field("host", "Hôte", "text", true, json!("")).placeholder("oracle.exemple.com"),
                field("port", "Port", "number", true, json!(1521)),
                field("service", "Nom de service", "text", true, json!("")).placeholder("FREEPDB1"),
                field("user", "Utilisateur", "text", true, json!("")),
                field("password", "Mot de passe", "password", true, json!("")),
            ],
        },
        Kind {
            kind: "smtp",
            label: "Serveur SMTP",
            description: "Envoi d'e-mails.",
            fields: vec![
                field("host", "Hôte", "text", true, json!("")).placeholder("smtp.exemple.com"),
                field("port", "Port", "number", true, json!(587)),
                field("security", "Sécurité", "select", true, json!("STARTTLS"))
                    .options(&["STARTTLS", "TLS", "aucune"]),
                field("user", "Utilisateur", "text", false, json!("")),
                field("password", "Mot de passe", "password", false, json!("")),
                field("from", "Expéditeur", "text", true, json!(""))
                    .placeholder("mini8n <robot@exemple.com>"),
            ],
        },
        Kind {
            kind: "chat_webhook",
            label: "Webhook de messagerie",
            description: "URL de webhook entrant Slack, Discord, Microsoft Teams, Mattermost ou Google Chat.",
            fields: vec![
                field("url", "URL du webhook", "password", true, json!(""))
                    .placeholder("https://hooks.slack.com/services/…"),
            ],
        },
    ]
}

fn kind(name: &str) -> AppResult<Kind> {
    kinds()
        .into_iter()
        .find(|k| k.kind == name)
        .ok_or_else(|| AppError::bad(format!("Type d'identifiant inconnu : {name}")))
}

/// Valide et normalise les champs ; `previous` fournit les secrets conservés.
fn normalize(
    kind: &Kind,
    input: &Map<String, Value>,
    previous: Option<&Map<String, Value>>,
) -> AppResult<Map<String, Value>> {
    let mut out = Map::new();
    for f in &kind.fields {
        let raw = input.get(f.name).cloned().unwrap_or(Value::Null);
        let text = match &raw {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            Value::Null => String::new(),
            _ => return Err(AppError::bad(format!("{} : valeur invalide", f.label))),
        };
        if text.len() > 2048 || text.chars().any(|c| c.is_control()) {
            return Err(AppError::bad(format!("{} : valeur invalide", f.label)));
        }
        let value = if text.is_empty() && f.secret {
            // Secret non ressaisi : on garde l'ancien.
            previous
                .and_then(|p| p.get(f.name))
                .cloned()
                .unwrap_or(json!(""))
        } else if f.ty == "number" {
            match text.parse::<u16>() {
                Ok(n) => json!(n),
                Err(_) if text.is_empty() => f.default.clone(),
                Err(_) => return Err(AppError::bad(format!("{} : nombre invalide", f.label))),
            }
        } else if f.ty == "select" && !text.is_empty() && !f.options.contains(&text.as_str()) {
            return Err(AppError::bad(format!("{} : valeur inconnue", f.label)));
        } else if text.is_empty() {
            f.default.clone()
        } else {
            json!(text)
        };
        let empty = value.as_str().is_some_and(str::is_empty) || value.is_null();
        if f.required && empty {
            return Err(AppError::bad(format!("{} est obligatoire", f.label)));
        }
        out.insert(f.name.into(), value);
    }
    if kind.kind == "chat_webhook" {
        let url = out.get("url").and_then(Value::as_str).unwrap_or_default();
        if !url.starts_with("https://") {
            return Err(AppError::bad(
                "L'URL du webhook doit commencer par https://",
            ));
        }
    }
    Ok(out)
}

fn summary(kind: &Kind, data: &Map<String, Value>) -> Value {
    let mut s = Map::new();
    for f in kind.fields.iter().filter(|f| !f.secret) {
        if let Some(v) = data.get(f.name) {
            s.insert(f.name.into(), v.clone());
        }
    }
    for f in kind.fields.iter().filter(|f| f.secret) {
        let set = data
            .get(f.name)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty());
        s.insert(format!("{}_defini", f.name), json!(set));
    }
    Value::Object(s)
}

/// Charge et déchiffre un identifiant du workspace (vérifie son type).
pub async fn load(
    db: &PgPool,
    vault: &Vault,
    ws: Uuid,
    id: &str,
    expected: &str,
) -> Result<Map<String, Value>, String> {
    let id = Uuid::parse_str(id.trim()).map_err(|_| "Choisissez un identifiant".to_string())?;
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT kind, secret FROM credentials WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(ws)
            .fetch_optional(db)
            .await
            .map_err(|e| e.to_string())?;
    let (kind, secret) = row.ok_or("Identifiant introuvable dans ce workspace")?;
    if kind != expected {
        return Err(format!(
            "Identifiant de type « {kind} » : « {expected} » attendu"
        ));
    }
    let plain = vault.decrypt(&secret, id.as_bytes())?;
    match serde_json::from_slice(&plain) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err("Identifiant illisible".into()),
    }
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

pub async fn types() -> Json<Vec<Kind>> {
    Json(kinds())
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInfo {
    id: Uuid,
    name: String,
    kind: String,
    summary: SqlJson<Value>,
    updated_at: DateTime<Utc>,
}

async fn info(db: &PgPool, ws: Uuid, id: Uuid) -> AppResult<CredentialInfo> {
    sqlx::query_as("SELECT id, name, kind, summary, updated_at FROM credentials WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(ws)
        .fetch_optional(db)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
) -> AppResult<Json<Vec<CredentialInfo>>> {
    require_member(&state.db, ws, user.id).await?;
    Ok(Json(
        sqlx::query_as("SELECT id, name, kind, summary, updated_at FROM credentials WHERE workspace_id = $1 ORDER BY name")
            .bind(ws)
            .fetch_all(&state.db)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBody {
    name: String,
    kind: String,
    data: Map<String, Value>,
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Json(body): Json<CreateBody>,
) -> AppResult<(StatusCode, Json<CredentialInfo>)> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let name = validate::label(&body.name, "Nom", 80)?;
    let kind = kind(&body.kind)?;
    let data = normalize(&kind, &body.data, None)?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM credentials WHERE workspace_id = $1")
        .bind(ws)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_CREDENTIALS {
        return Err(AppError::bad(format!(
            "Limite de {MAX_CREDENTIALS} identifiants atteinte"
        )));
    }
    let id = Uuid::new_v4();
    let secret = state
        .vault
        .encrypt(
            &serde_json::to_vec(&data).map_err(AppError::internal)?,
            id.as_bytes(),
        )
        .map_err(AppError::internal)?;
    let res = sqlx::query(
        "INSERT INTO credentials (id, workspace_id, name, kind, secret, summary, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(ws)
    .bind(&name)
    .bind(kind.kind)
    .bind(secret)
    .bind(SqlJson(summary(&kind, &data)))
    .bind(user.id)
    .execute(&state.db)
    .await;
    if let Err(sqlx::Error::Database(e)) = &res
        && e.code().as_deref() == Some("23505")
    {
        return Err(AppError::Conflict(
            "Un identifiant porte déjà ce nom".into(),
        ));
    }
    res?;
    tracing::info!(workspace_id = %ws, credential_id = %id, kind = kind.kind, by = %user.id, "identifiant créé");
    Ok((StatusCode::CREATED, Json(info(&state.db, ws, id).await?)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBody {
    name: String,
    data: Map<String, Value>,
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateBody>,
) -> AppResult<Json<CredentialInfo>> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let name = validate::label(&body.name, "Nom", 80)?;
    let current = info(&state.db, ws, id).await?;
    let kind = kind(&current.kind)?;
    let previous = load(&state.db, &state.vault, ws, &id.to_string(), kind.kind)
        .await
        .map_err(AppError::internal)?;
    let data = normalize(&kind, &body.data, Some(&previous))?;
    let secret = state
        .vault
        .encrypt(
            &serde_json::to_vec(&data).map_err(AppError::internal)?,
            id.as_bytes(),
        )
        .map_err(AppError::internal)?;
    let res = sqlx::query("UPDATE credentials SET name = $3, secret = $4, summary = $5, updated_at = now() WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(ws)
        .bind(&name)
        .bind(secret)
        .bind(SqlJson(summary(&kind, &data)))
        .execute(&state.db)
        .await;
    if let Err(sqlx::Error::Database(e)) = &res
        && e.code().as_deref() == Some("23505")
    {
        return Err(AppError::Conflict(
            "Un identifiant porte déjà ce nom".into(),
        ));
    }
    res?;
    Ok(Json(info(&state.db, ws, id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_member(&state.db, ws, user.id).await?;
    let n = sqlx::query("DELETE FROM credentials WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(ws)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    tracing::info!(workspace_id = %ws, credential_id = %id, by = %user.id, "identifiant supprimé");
    Ok(StatusCode::NO_CONTENT)
}

/// Vérifie la connexion (bases de données, SMTP).
pub async fn test(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let current = info(&state.db, ws, id).await?;
    let ctx = crate::integrations::Ctx::new(&state, ws, None);
    match crate::integrations::test_credential(&ctx, &current.kind, &id.to_string()).await {
        Ok(message) => Ok(Json(json!({ "ok": true, "message": message }))),
        Err(e) => Ok(Json(json!({ "ok": false, "message": e }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn normalization_keeps_secrets_and_validates() {
        let pg = kind("postgres").unwrap();
        let first = normalize(&pg, &obj(json!({"host": "db", "port": "5433", "database": "app", "user": "u", "password": "s3cret"})), None).unwrap();
        assert_eq!(first["port"], 5433);
        assert_eq!(first["ssl"], "préféré");
        // Secret vide à la modification : conservé.
        let second = normalize(&pg, &obj(json!({"host": "db2", "port": 5432, "database": "app", "user": "u", "password": ""})), Some(&first)).unwrap();
        assert_eq!(second["password"], "s3cret");
        let s = summary(&pg, &second);
        assert!(s.get("password").is_none());
        assert_eq!(s["password_defini"], true);
        assert!(
            normalize(
                &pg,
                &obj(json!({"host": "", "database": "a", "user": "u"})),
                None
            )
            .is_err()
        );
        assert!(
            normalize(
                &pg,
                &obj(json!({"host": "h", "port": "x", "database": "a", "user": "u"})),
                None
            )
            .is_err()
        );
        let chat = kind("chat_webhook").unwrap();
        assert!(normalize(&chat, &obj(json!({"url": "http://insecure"})), None).is_err());
    }
}
