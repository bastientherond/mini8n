//! Parcours de compte par e-mail : mot de passe oublié, vérification de l'adresse,
//! invitations dans un workspace.
//!
//! - Jetons aléatoires de 256 bits à usage unique, à durée limitée ; seul leur hash est
//!   stocké (table `email_tokens`).
//! - « Mot de passe oublié » répond toujours de la même façon, que le compte existe ou non,
//!   et l'e-mail part en tâche de fond (pas de déduction des adresses inscrites).
//! - Une réinitialisation déconnecte toutes les sessions et débloque le compte.
//! - Inscriptions fermées par défaut : on rejoint un workspace par invitation. Seule une
//!   adresse vérifiée peut être ajoutée directement : sinon, quiconque s'inscrirait avec
//!   l'adresse d'un collègue recevrait l'accès destiné à ce collègue.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::AppState;
use crate::auth::{AuthUser, create_session_cookie, hash_password, new_token, token_hash};
use crate::error::{AppError, AppResult};
use crate::security::ClientIp;
use crate::validate;
use crate::workspaces::{Role, require_owner};

const RESET_TTL: Duration = Duration::minutes(30);
const VERIFY_TTL: Duration = Duration::hours(24);
const INVITE_TTL: Duration = Duration::days(7);
const MAX_PENDING_INVITES: i64 = 100;

fn link(state: &AppState, path: &str) -> String {
    let origin = state
        .config
        .public_origins
        .first()
        .map(String::as_str)
        .unwrap_or("http://localhost:8080");
    format!("{origin}/#/{path}")
}

/// Jeton à usage unique : (jeton en clair, identifiant de la ligne).
async fn issue(
    state: &AppState,
    purpose: &str,
    email: &str,
    user: Option<Uuid>,
    ttl: Duration,
) -> AppResult<String> {
    let token = new_token()?;
    sqlx::query(
        "INSERT INTO email_tokens (id, token_hash, purpose, email_normalized, user_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(token_hash(&token))
    .bind(purpose)
    .bind(email)
    .bind(user)
    .bind(Utc::now() + ttl)
    .execute(&state.db)
    .await?;
    Ok(token)
}

fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---------------------------------------------------------------------------
// Mot de passe oublié
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgotBody {
    email: String,
}

/// `POST /api/auth/password/forgot` : réponse identique que le compte existe ou non.
pub async fn forgot_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<ForgotBody>,
) -> AppResult<StatusCode> {
    state.limits.login_ip.limit(&ip)?;
    let Some(mailer) = state.mailer.clone() else {
        return Err(AppError::bad(
            "La réinitialisation par e-mail n'est pas configurée : contactez l'administrateur",
        ));
    };
    let Ok((_, email)) = validate::email(&body.email) else {
        return Ok(StatusCode::ACCEPTED);
    };
    // Limite par adresse sans révéler son existence (même réponse au-delà).
    if state.limits.emails.check(&email).is_err() {
        return Ok(StatusCode::ACCEPTED);
    }
    tokio::spawn(async move {
        let user: Result<Option<(Uuid, String, String)>, _> =
            sqlx::query_as("SELECT id, email, display_name FROM users WHERE email_normalized = $1")
                .bind(&email)
                .fetch_optional(&state.db)
                .await;
        let Ok(Some((id, address, name))) = user else {
            return;
        };
        match issue(&state, "reset", &email, Some(id), RESET_TTL).await {
            Ok(token) => mailer.send_later(
                &address,
                "Réinitialisation de votre mot de passe mini8n".into(),
                format!(
                    "Bonjour {name},\n\nPour choisir un nouveau mot de passe, ouvrez ce lien (valable 30 minutes, une seule fois) :\n\n{}\n\n\
                     Si vous n'êtes pas à l'origine de cette demande, ignorez cet e-mail : votre mot de passe reste inchangé.\n",
                    link(&state, &format!("reinitialiser/{token}"))
                ),
            ),
            Err(e) => tracing::error!(%e, "jeton de réinitialisation"),
        }
    });
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetBody {
    token: String,
    password: String,
}

/// `POST /api/auth/password/reset` : nouveau mot de passe ; toutes les sessions sont fermées.
pub async fn reset_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<ResetBody>,
) -> AppResult<StatusCode> {
    state.limits.login_ip.limit(&ip)?;
    let invalid = || AppError::bad("Lien invalide ou expiré : refaites une demande");
    if !valid_token(&body.token) {
        return Err(invalid());
    }
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT u.id, u.email_normalized FROM email_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1 AND t.purpose = 'reset' AND t.expires_at > now()
           AND u.email_normalized = t.email_normalized",
    )
    .bind(token_hash(&body.token))
    .fetch_optional(&state.db)
    .await?;
    let (user, email) = row.ok_or_else(invalid)?;
    validate::password(&body.password, &email)?;
    let hash = hash_password(&state, body.password).await?;
    let mut tx = state.db.begin().await?;
    // Jeton consommé dans la même transaction : un second usage échoue.
    let consumed =
        sqlx::query("DELETE FROM email_tokens WHERE token_hash = $1 AND purpose = 'reset'")
            .bind(token_hash(&body.token))
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if consumed == 0 {
        return Err(invalid());
    }
    sqlx::query(
        "UPDATE users SET password_hash = $2, password_changed_at = now(), failed_logins = 0,
                locked_until = NULL, email_verified_at = COALESCE(email_verified_at, now())
         WHERE id = $1",
    )
    .bind(user)
    .bind(hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM email_tokens WHERE user_id = $1 AND purpose = 'reset'")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    // Reprise de compte : les jetons d'IA créés par un éventuel intrus sont révoqués aussi.
    let revoked: Vec<Uuid> =
        sqlx::query_scalar("DELETE FROM api_tokens WHERE user_id = $1 RETURNING id")
            .bind(user)
            .fetch_all(&mut *tx)
            .await?;
    tx.commit().await?;
    for token in revoked {
        state.mcp.forget_token(&state.live, token);
    }
    tracing::info!(user_id = %user, "mot de passe réinitialisé");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Vérification de l'adresse
// ---------------------------------------------------------------------------

/// `POST /api/auth/email/send-verification` : envoie le lien à l'utilisateur connecté.
pub async fn send_verification(
    State(state): State<AppState>,
    user: AuthUser,
) -> AppResult<StatusCode> {
    let Some(mailer) = state.mailer.clone() else {
        return Err(AppError::bad(
            "L'envoi d'e-mails n'est pas configuré sur ce serveur",
        ));
    };
    let email = user.email.to_lowercase();
    state.limits.emails.limit(&email)?;
    let verified: bool =
        sqlx::query_scalar("SELECT email_verified_at IS NOT NULL FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
    if verified {
        return Err(AppError::Conflict("Votre adresse est déjà vérifiée".into()));
    }
    let token = issue(&state, "verify", &email, Some(user.id), VERIFY_TTL).await?;
    mailer.send_later(
        &user.email,
        "Vérifiez votre adresse e-mail mini8n".into(),
        format!(
            "Bonjour {},\n\nPour confirmer votre adresse, ouvrez ce lien en étant connecté à votre compte mini8n (valable 24 heures) :\n\n{}\n",
            user.name,
            link(&state, &format!("verifier/{token}"))
        ),
    );
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenBody {
    token: String,
}

/// `POST /api/auth/email/verify` : le lien reçu prouve la possession de l'adresse. Il doit
/// être ouvert depuis le compte concerné : sinon, quelqu'un qui s'inscrirait avec l'adresse
/// d'autrui ferait vérifier son propre compte par le simple clic (ou l'antivirus) du
/// destinataire, puis serait ajouté directement aux workspaces destinés à cette personne.
pub async fn verify_email(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: AuthUser,
    Json(body): Json<TokenBody>,
) -> AppResult<StatusCode> {
    state.limits.login_ip.limit(&ip)?;
    let invalid = || AppError::bad("Lien invalide ou expiré");
    if !valid_token(&body.token) {
        return Err(invalid());
    }
    let mut tx = state.db.begin().await?;
    let user: Option<Uuid> = sqlx::query_scalar(
        "DELETE FROM email_tokens t USING users u
         WHERE t.token_hash = $1 AND t.purpose = 'verify' AND t.expires_at > now()
           AND u.id = t.user_id AND u.email_normalized = t.email_normalized AND t.user_id = $2
         RETURNING t.user_id",
    )
    .bind(token_hash(&body.token))
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let user = user.ok_or_else(invalid)?;
    sqlx::query(
        "UPDATE users SET email_verified_at = COALESCE(email_verified_at, now()) WHERE id = $1",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM email_tokens WHERE user_id = $1 AND purpose = 'verify'")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Invitations
// ---------------------------------------------------------------------------

/// Invitation créée par un propriétaire.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invitation {
    id: Uuid,
    email: String,
    role: String,
    expires_at: DateTime<Utc>,
    /// Envoyée par e-mail ; sinon `link` est à transmettre à la main.
    delivered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
}

/// Invite `email` dans le workspace (remplace une invitation en attente pour cette adresse).
pub async fn invite(
    state: &AppState,
    ws: Uuid,
    by: &AuthUser,
    email: &str,
    role: Role,
) -> AppResult<Invitation> {
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_tokens WHERE workspace_id = $1 AND purpose = 'invite' AND expires_at > now()",
    )
    .bind(ws)
    .fetch_one(&state.db)
    .await?;
    if pending >= MAX_PENDING_INVITES {
        return Err(AppError::bad(format!(
            "{MAX_PENDING_INVITES} invitations en attente au maximum"
        )));
    }
    let workspace: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
        .bind(ws)
        .fetch_one(&state.db)
        .await?;
    let token = new_token()?;
    let id = Uuid::new_v4();
    let expires_at = Utc::now() + INVITE_TTL;
    let delivered = state.mailer.is_some();
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM email_tokens WHERE workspace_id = $1 AND email_normalized = $2 AND purpose = 'invite'")
        .bind(ws)
        .bind(email)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO email_tokens (id, token_hash, purpose, email_normalized, workspace_id, role, invited_by, delivered, expires_at)
         VALUES ($1, $2, 'invite', $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(token_hash(&token))
    .bind(email)
    .bind(ws)
    .bind(role.as_str())
    .bind(by.id)
    .bind(delivered)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let url = link(state, &format!("invitation/{token}"));
    if let Some(mailer) = &state.mailer {
        mailer.send_later(
            email,
            format!("{} vous invite sur mini8n", by.name),
            format!(
                "Bonjour,\n\n{} vous invite à rejoindre le workspace « {workspace} » sur mini8n.\n\n\
                 Pour accepter (lien valable 7 jours) :\n\n{url}\n\n\
                 Si vous ne connaissez pas cette personne, ignorez cet e-mail.\n",
                by.name
            ),
        );
    }
    tracing::info!(workspace_id = %ws, by = %by.id, "invitation créée");
    Ok(Invitation {
        id,
        email: email.to_string(),
        role: role.as_str().to_string(),
        expires_at,
        delivered,
        // Sans SMTP, le propriétaire transmet le lien lui-même.
        link: (!delivered).then_some(url),
    })
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvitation {
    id: Uuid,
    email: String,
    role: String,
    invited_by: Option<String>,
    delivered: bool,
    expires_at: DateTime<Utc>,
}

/// `GET /api/workspaces/{ws}/invitations` (propriétaires).
pub async fn list_invitations(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
) -> AppResult<Json<Vec<PendingInvitation>>> {
    require_owner(&state.db, ws, user.id).await?;
    Ok(Json(
        sqlx::query_as(
            "SELECT t.id, t.email_normalized AS email, t.role, u.display_name AS invited_by, t.delivered, t.expires_at
             FROM email_tokens t LEFT JOIN users u ON u.id = t.invited_by
             WHERE t.workspace_id = $1 AND t.purpose = 'invite' AND t.expires_at > now()
             ORDER BY t.created_at DESC",
        )
        .bind(ws)
        .fetch_all(&state.db)
        .await?,
    ))
}

/// `DELETE /api/workspaces/{ws}/invitations/{id}` (propriétaires).
pub async fn revoke_invitation(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_owner(&state.db, ws, user.id).await?;
    let done = sqlx::query(
        "DELETE FROM email_tokens WHERE id = $1 AND workspace_id = $2 AND purpose = 'invite'",
    )
    .bind(id)
    .bind(ws)
    .execute(&state.db)
    .await?
    .rows_affected();
    if done == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct InviteRow {
    email_normalized: String,
    workspace_id: Uuid,
    role: String,
    delivered: bool,
    workspace: String,
    invited_by: Option<String>,
    inviter_id: Option<Uuid>,
    expires_at: DateTime<Utc>,
}

async fn find_invite(state: &AppState, token: &str) -> AppResult<InviteRow> {
    let invalid = || AppError::bad("Invitation invalide, expirée ou déjà utilisée");
    if !valid_token(token) {
        return Err(invalid());
    }
    sqlx::query_as(
        "SELECT t.email_normalized, t.workspace_id, t.role, t.delivered, w.name AS workspace,
                u.display_name AS invited_by, t.invited_by AS inviter_id, t.expires_at
         FROM email_tokens t JOIN workspaces w ON w.id = t.workspace_id
         LEFT JOIN users u ON u.id = t.invited_by
         WHERE t.token_hash = $1 AND t.purpose = 'invite' AND t.expires_at > now()
           AND EXISTS (SELECT 1 FROM workspace_members m
                       WHERE m.workspace_id = t.workspace_id AND m.user_id = t.invited_by AND m.role = 'owner')",
    )
    .bind(token_hash(token))
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(invalid)
}

/// `GET /api/auth/invitations/{token}` : détail affiché à la personne invitée.
pub async fn show_invitation(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(token): Path<String>,
) -> AppResult<Json<Value>> {
    state.limits.public_ip.limit(&ip)?;
    let inv = find_invite(&state, &token).await?;
    let account: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email_normalized = $1)")
            .bind(&inv.email_normalized)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(json!({
        "workspace": inv.workspace,
        "email": inv.email_normalized,
        "role": inv.role,
        "invitedBy": inv.invited_by,
        "expiresAt": inv.expires_at,
        "accountExists": account,
    })))
}

/// Ajoute le membre et consomme l'invitation (même transaction).
async fn join(
    tx: &mut sqlx::PgConnection,
    token: &str,
    inv: &InviteRow,
    user: Uuid,
) -> AppResult<()> {
    let consumed =
        sqlx::query("DELETE FROM email_tokens WHERE token_hash = $1 AND purpose = 'invite'")
            .bind(token_hash(token))
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if consumed == 0 {
        return Err(AppError::bad("Invitation déjà utilisée"));
    }
    // Invitation toujours valable : son auteur est encore propriétaire (un propriétaire
    // retiré ou rétrogradé ne peut pas laisser d'accès derrière lui) et il reste de la place.
    let (owner, members): (bool, i64) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND role = 'owner'),
                (SELECT count(*) FROM workspace_members WHERE workspace_id = $1)",
    )
    .bind(inv.workspace_id)
    .bind(inv.inviter_id)
    .fetch_one(&mut *tx)
    .await?;
    if !owner {
        return Err(AppError::bad(
            "Cette invitation n'est plus valable : demandez-en une nouvelle",
        ));
    }
    if members >= crate::workspaces::MAX_MEMBERS {
        return Err(AppError::bad(
            "Ce workspace a atteint son nombre maximal de membres",
        ));
    }
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(inv.workspace_id)
    .bind(user)
    .bind(&inv.role)
    .execute(&mut *tx)
    .await?;
    // Invitation reçue par e-mail : l'adresse est prouvée.
    if inv.delivered {
        sqlx::query(
            "UPDATE users SET email_verified_at = COALESCE(email_verified_at, now()) WHERE id = $1",
        )
        .bind(user)
        .execute(&mut *tx)
        .await?;
    }
    Ok(())
}

/// `POST /api/auth/invitations/{token}/accept` : compte existant, connecté avec l'adresse invitée.
pub async fn accept_invitation(
    State(state): State<AppState>,
    user: AuthUser,
    Path(token): Path<String>,
) -> AppResult<Json<Value>> {
    let inv = find_invite(&state, &token).await?;
    if user.email.to_lowercase() != inv.email_normalized {
        return Err(AppError::Forbidden(format!(
            "Cette invitation est destinée à {} : connectez-vous avec ce compte",
            inv.email_normalized
        )));
    }
    let mut tx = state.db.begin().await?;
    join(&mut tx, &token, &inv, user.id).await?;
    tx.commit().await?;
    tracing::info!(workspace_id = %inv.workspace_id, user_id = %user.id, "invitation acceptée");
    Ok(Json(json!({ "workspaceId": inv.workspace_id })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InviteRegisterBody {
    name: String,
    password: String,
}

/// `POST /api/auth/invitations/{token}/register` : création du compte de la personne invitée
/// (possible même inscriptions fermées), puis connexion.
pub async fn register_invited(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(token): Path<String>,
    Json(body): Json<InviteRegisterBody>,
) -> AppResult<Response> {
    state.limits.register_ip.limit(&ip)?;
    let inv = find_invite(&state, &token).await?;
    let name = validate::label(&body.name, "Nom", 80)?;
    validate::password(&body.password, &inv.email_normalized)?;
    let hash = hash_password(&state, body.password).await?;
    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO users (id, email, email_normalized, display_name, password_hash)
         VALUES ($1, $2, $2, $3, $4) ON CONFLICT (email_normalized) DO NOTHING",
    )
    .bind(id)
    .bind(&inv.email_normalized)
    .bind(&name)
    .bind(&hash)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(AppError::Conflict(
            "Un compte utilise déjà cette adresse : connectez-vous pour accepter l'invitation"
                .into(),
        ));
    }
    join(&mut tx, &token, &inv, id).await?;
    tx.commit().await?;
    tracing::info!(user_id = %id, workspace_id = %inv.workspace_id, "compte créé sur invitation");
    let cookie = create_session_cookie(&state, &headers, id, &ip).await?;
    let mut res = (
        StatusCode::CREATED,
        Json(json!({ "workspaceId": inv.workspace_id })),
    )
        .into_response();
    res.headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie);
    Ok(res)
}

/// Purge des jetons expirés (avec celle des sessions).
pub async fn purge_expired(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM email_tokens WHERE expires_at <= now()")
            .execute(db)
            .await?
            .rows_affected(),
    )
}
