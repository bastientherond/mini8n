//! Workspaces et gestion de leurs membres.
//!
//! Rôles : `owner` (gère le workspace et ses membres) et `member` (édite les workflows).
//! Un non-membre reçoit 404 : l'existence d'un workspace n'est jamais révélée.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::validate;

const MAX_WORKSPACES_PER_USER: i64 = 50;
const MAX_MEMBERS: i64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Member,
}

impl Role {
    pub(crate) fn parse(s: &str) -> AppResult<Self> {
        match s {
            "owner" => Ok(Role::Owner),
            "member" => Ok(Role::Member),
            _ => Err(AppError::bad("Rôle invalide (owner ou member)")),
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Member => "member",
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    id: Uuid,
    name: String,
    role: String,
    member_count: i64,
    created_at: DateTime<Utc>,
}

pub async fn list_for_user(db: &PgPool, user_id: Uuid) -> AppResult<Vec<WorkspaceSummary>> {
    Ok(sqlx::query_as(
        "SELECT w.id, w.name, m.role, w.created_at,
                (SELECT count(*) FROM workspace_members x WHERE x.workspace_id = w.id) AS member_count
         FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id
         WHERE m.user_id = $1 ORDER BY w.created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?)
}

async fn summary(db: &PgPool, ws: Uuid, user_id: Uuid) -> AppResult<WorkspaceSummary> {
    list_for_user(db, user_id)
        .await?
        .into_iter()
        .find(|w| w.id == ws)
        .ok_or(AppError::NotFound)
}

/// Rôle de l'utilisateur dans le workspace ; 404 s'il n'en est pas membre.
pub async fn require_member(db: &PgPool, ws: Uuid, user_id: Uuid) -> AppResult<Role> {
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(ws)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Role::parse(&role.ok_or(AppError::NotFound)?)
}

pub(crate) async fn require_owner(db: &PgPool, ws: Uuid, user_id: Uuid) -> AppResult<()> {
    match require_member(db, ws, user_id).await? {
        Role::Owner => Ok(()),
        Role::Member => Err(AppError::Forbidden(
            "Action réservée aux propriétaires du workspace".into(),
        )),
    }
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<Json<Vec<WorkspaceSummary>>> {
    Ok(Json(list_for_user(&state.db, user.id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameBody {
    name: String,
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<NameBody>,
) -> AppResult<(StatusCode, Json<WorkspaceSummary>)> {
    state.limits.writes.limit(&user.id.to_string())?;
    let name = validate::label(&body.name, "Nom du workspace", 80)?;
    let owned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_members WHERE user_id = $1 AND role = 'owner'",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if owned >= MAX_WORKSPACES_PER_USER {
        return Err(AppError::bad(format!(
            "Limite de {MAX_WORKSPACES_PER_USER} workspaces atteinte"
        )));
    }
    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO workspaces (id, name, created_by) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(&name)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!(workspace_id = %id, user_id = %user.id, "workspace créé");
    Ok((
        StatusCode::CREATED,
        Json(summary(&state.db, id, user.id).await?),
    ))
}

pub async fn rename(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Json(body): Json<NameBody>,
) -> AppResult<Json<WorkspaceSummary>> {
    require_owner(&state.db, ws, user.id).await?;
    let name = validate::label(&body.name, "Nom du workspace", 80)?;
    sqlx::query("UPDATE workspaces SET name = $2 WHERE id = $1")
        .bind(ws)
        .bind(name)
        .execute(&state.db)
        .await?;
    Ok(Json(summary(&state.db, ws, user.id).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
) -> AppResult<StatusCode> {
    require_owner(&state.db, ws, user.id).await?;
    sqlx::query("DELETE FROM workspaces WHERE id = $1")
        .bind(ws)
        .execute(&state.db)
        .await?;
    state.live.revoke(ws, None);
    tracing::info!(workspace_id = %ws, user_id = %user.id, "workspace supprimé");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Membres
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    user_id: Uuid,
    email: String,
    name: String,
    role: String,
    created_at: DateTime<Utc>,
}

async fn member_list(db: &PgPool, ws: Uuid) -> AppResult<Vec<Member>> {
    Ok(sqlx::query_as(
        "SELECT u.id AS user_id, u.email, u.display_name AS name, m.role, m.created_at
         FROM workspace_members m JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = $1 ORDER BY m.created_at",
    )
    .bind(ws)
    .fetch_all(db)
    .await?)
}

pub async fn members(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
) -> AppResult<Json<Vec<Member>>> {
    require_member(&state.db, ws, user.id).await?;
    Ok(Json(member_list(&state.db, ws).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddMemberBody {
    email: String,
    role: String,
}

#[derive(Serialize)]
pub struct AddMemberResponse {
    members: Vec<Member>,
    /// Invitation créée quand la personne n'a pas de compte (ou pas d'adresse vérifiée).
    #[serde(skip_serializing_if = "Option::is_none")]
    invitation: Option<crate::accounts::Invitation>,
}

/// Ajoute un membre : directement si son compte a une adresse vérifiée, sinon par une
/// invitation qu'il doit accepter (quiconque peut s'inscrire avec n'importe quelle adresse :
/// l'ajouter directement donnerait l'accès à un inconnu).
pub async fn add_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Json(body): Json<AddMemberBody>,
) -> AppResult<(StatusCode, Json<AddMemberResponse>)> {
    require_owner(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    let role = Role::parse(&body.role)?;
    let (_, email) = validate::email(&body.email)?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_members WHERE workspace_id = $1")
            .bind(ws)
            .fetch_one(&state.db)
            .await?;
    if count >= MAX_MEMBERS {
        return Err(AppError::bad(format!(
            "Limite de {MAX_MEMBERS} membres atteinte"
        )));
    }
    let target: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT id, email_verified_at IS NOT NULL FROM users WHERE email_normalized = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    let target = match target {
        Some((id, true)) => id,
        _ => {
            let member: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM workspace_members m JOIN users u ON u.id = m.user_id
                 WHERE m.workspace_id = $1 AND u.email_normalized = $2)",
            )
            .bind(ws)
            .bind(&email)
            .fetch_one(&state.db)
            .await?;
            if member {
                return Err(AppError::Conflict(
                    "Cette personne est déjà membre du workspace".into(),
                ));
            }
            let invitation = crate::accounts::invite(&state, ws, &user, &email, role).await?;
            return Ok((
                StatusCode::ACCEPTED,
                Json(AddMemberResponse {
                    members: member_list(&state.db, ws).await?,
                    invitation: Some(invitation),
                }),
            ));
        }
    };
    let inserted = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(ws)
    .bind(target)
    .bind(role.as_str())
    .execute(&state.db)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(AppError::Conflict(
            "Cette personne est déjà membre du workspace".into(),
        ));
    }
    tracing::info!(workspace_id = %ws, by = %user.id, member = %target, "membre ajouté");
    Ok((
        StatusCode::CREATED,
        Json(AddMemberResponse {
            members: member_list(&state.db, ws).await?,
            invitation: None,
        }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleBody {
    role: String,
}

const LAST_OWNER: &str = "Le workspace doit conserver au moins un propriétaire";

pub async fn update_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, member)): Path<(Uuid, Uuid)>,
    Json(body): Json<RoleBody>,
) -> AppResult<Json<Vec<Member>>> {
    require_owner(&state.db, ws, user.id).await?;
    let role = Role::parse(&body.role)?;
    let mut tx = state.db.begin().await?;
    // Verrou sur le workspace : sérialise les changements de rôles concurrents.
    sqlx::query("SELECT id FROM workspaces WHERE id = $1 FOR UPDATE")
        .bind(ws)
        .execute(&mut *tx)
        .await?;
    let current: Option<String> = sqlx::query_scalar(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(ws)
    .bind(member)
    .fetch_optional(&mut *tx)
    .await?;
    let current = Role::parse(&current.ok_or(AppError::NotFound)?)?;
    if current == Role::Owner && role == Role::Member {
        let owners: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workspace_members WHERE workspace_id = $1 AND role = 'owner'",
        )
        .bind(ws)
        .fetch_one(&mut *tx)
        .await?;
        if owners <= 1 {
            return Err(AppError::Conflict(LAST_OWNER.into()));
        }
    }
    sqlx::query("UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2")
        .bind(ws)
        .bind(member)
        .bind(role.as_str())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(member_list(&state.db, ws).await?))
}

/// Retire un membre (propriétaire) ou quitte le workspace (soi-même).
pub async fn remove_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, member)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    if member != user.id {
        require_owner(&state.db, ws, user.id).await?;
    } else {
        require_member(&state.db, ws, user.id).await?;
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM workspaces WHERE id = $1 FOR UPDATE")
        .bind(ws)
        .execute(&mut *tx)
        .await?;
    let removed = sqlx::query(
        "DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2
         AND (role <> 'owner' OR (SELECT count(*) FROM workspace_members WHERE workspace_id = $1 AND role = 'owner') > 1)",
    )
    .bind(ws)
    .bind(member)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if removed == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2)")
                .bind(ws)
                .bind(member)
                .fetch_one(&mut *tx)
                .await?;
        return Err(if exists {
            AppError::Conflict(LAST_OWNER.into())
        } else {
            AppError::NotFound
        });
    }
    tx.commit().await?;
    state.live.revoke(ws, Some(member));
    Ok(StatusCode::NO_CONTENT)
}
