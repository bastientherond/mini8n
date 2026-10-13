//! Jetons d'accès personnels : permettent à une IA (Claude Code, Claude Desktop…) de se
//! connecter au serveur MCP (`/api/mcp`) au nom d'un membre, sur un seul workspace.
//!
//! - Le jeton (`m8n_…`) n'est affiché qu'à sa création ; seul son hash SHA-256 est stocké.
//! - L'appartenance au workspace est revérifiée à chaque utilisation (membre retiré =
//!   jeton inutilisable).
//! - Chacun ne voit et ne révoque que ses propres jetons.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::{AuthUser, new_token, token_hash};
use crate::error::{AppError, AppResult};
use crate::validate;
use crate::workspaces::require_member;

const PREFIX: &str = "m8n_";
const MAX_TOKENS: i64 = 20;

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TokenSummary {
    id: Uuid,
    name: String,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
) -> AppResult<Json<Vec<TokenSummary>>> {
    require_member(&state.db, ws, user.id).await?;
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name, created_at, last_used_at, expires_at FROM api_tokens
             WHERE workspace_id = $1 AND user_id = $2 ORDER BY created_at DESC",
        )
        .bind(ws)
        .bind(user.id)
        .fetch_all(&state.db)
        .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CreateBody {
    name: String,
    /// Durée de validité en jours (`null` : sans expiration).
    expires_in_days: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    #[serde(flatten)]
    summary: TokenSummary,
    /// Jeton en clair : affiché une seule fois.
    token: String,
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Json(body): Json<CreateBody>,
) -> AppResult<Json<Created>> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let name = validate::label(&body.name, "Nom du jeton", 60)?;
    let expires_at = match body.expires_in_days {
        None => None,
        Some(d) if (1..=365).contains(&d) => Some(Utc::now() + Duration::days(d)),
        Some(_) => return Err(AppError::bad("Validité : de 1 à 365 jours")),
    };
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM api_tokens WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(ws)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if count >= MAX_TOKENS {
        return Err(AppError::bad(format!(
            "{MAX_TOKENS} jetons au maximum : révoquez-en un"
        )));
    }
    let token = format!("{PREFIX}{}", new_token()?);
    let summary: TokenSummary = sqlx::query_as(
        "INSERT INTO api_tokens (id, token_hash, user_id, workspace_id, name, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, name, created_at, last_used_at, expires_at",
    )
    .bind(Uuid::new_v4())
    .bind(token_hash(&token))
    .bind(user.id)
    .bind(ws)
    .bind(name)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(Created { summary, token }))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<serde_json::Value>> {
    require_member(&state.db, ws, user.id).await?;
    let done =
        sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND workspace_id = $2 AND user_id = $3")
            .bind(id)
            .bind(ws)
            .bind(user.id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if done == 0 {
        return Err(AppError::NotFound);
    }
    state.mcp.forget_token(&state.live, id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Identité d'un appel authentifié par jeton.
#[derive(Debug, Clone)]
pub struct TokenAuth {
    pub token_id: Uuid,
    /// Nom du jeton (affiché aux autres membres : « Claude »…).
    pub name: String,
    pub user_id: Uuid,
    pub user_name: String,
    pub workspace_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    token_id: Uuid,
    name: String,
    user_id: Uuid,
    user_name: String,
    workspace_id: Uuid,
    last_used_at: Option<DateTime<Utc>>,
}

/// Authentifie un en-tête `Authorization: Bearer m8n_…`.
pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> AppResult<TokenAuth> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| t.starts_with(PREFIX) && t.len() <= 128)
        .ok_or(AppError::Unauthorized)?;
    let row: Option<TokenRow> = sqlx::query_as(
        "SELECT t.id AS token_id, t.name, u.id AS user_id, u.display_name AS user_name, t.workspace_id, t.last_used_at
         FROM api_tokens t
         JOIN users u ON u.id = t.user_id
         JOIN workspace_members m ON m.workspace_id = t.workspace_id AND m.user_id = t.user_id
         WHERE t.token_hash = $1 AND (t.expires_at IS NULL OR t.expires_at > now())",
    )
    .bind(token_hash(token))
    .fetch_optional(&state.db)
    .await?;
    let TokenRow {
        token_id,
        name,
        user_id,
        user_name,
        workspace_id,
        last_used_at: last_used,
    } = row.ok_or(AppError::Unauthorized)?;
    if last_used.is_none_or(|t| Utc::now() - t > Duration::minutes(5)) {
        sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1")
            .bind(token_id)
            .execute(&state.db)
            .await?;
    }
    Ok(TokenAuth {
        token_id,
        name,
        user_id,
        user_name,
        workspace_id,
    })
}
