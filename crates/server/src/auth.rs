//! Comptes et sessions.
//!
//! - Mots de passe : Argon2id (paramètres OWASP), calcul hors du runtime async
//!   et nombre de calculs simultanés limité.
//! - Sessions : jeton aléatoire de 256 bits dans un cookie `HttpOnly; Secure;
//!   SameSite=Strict` ; seul son hash SHA-256 est stocké en base.
//! - Connexion : message d'erreur générique, temps constant pour un compte
//!   inexistant, limitation par IP et par compte, blocage progressif.

use std::sync::{Arc, LazyLock};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;
use crate::config::CookieSecure;
use crate::error::{AppError, AppResult};
use crate::security::{ClientIp, user_agent};
use crate::validate;
use crate::workspaces::{WorkspaceSummary, list_for_user};

const MAX_SESSIONS_PER_USER: i64 = 20;
const LOCK_AFTER_FAILURES: i32 = 5;

fn argon2() -> Argon2<'static> {
    // OWASP : Argon2id, m = 19 MiB, t = 2, p = 1.
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19 * 1024, 2, 1, None).expect("paramètres Argon2 valides"),
    )
}

/// Hash factice pour que la vérification d'un compte inexistant prenne le même temps.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(b"mini8n-dummy-password", &salt)
        .expect("hash factice")
        .to_string()
});

pub async fn hash_password(state: &AppState, password: String) -> AppResult<String> {
    let _permit = state
        .hash_permits
        .acquire()
        .await
        .map_err(AppError::internal)?;
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        argon2()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(AppError::internal)
    })
    .await
    .map_err(AppError::internal)?
}

pub async fn verify_password(
    state: &AppState,
    password: String,
    hash: Option<String>,
) -> AppResult<bool> {
    let _permit = state
        .hash_permits
        .acquire()
        .await
        .map_err(AppError::internal)?;
    tokio::task::spawn_blocking(move || {
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| DUMMY_HASH.clone());
        let parsed = PasswordHash::new(&hash).map_err(AppError::internal)?;
        let ok = argon2()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok();
        Ok(ok && known)
    })
    .await
    .map_err(AppError::internal)?
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

pub(crate) fn new_token() -> AppResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(AppError::internal)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub(crate) fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

const SECURE_COOKIE: &str = "__Host-mini8n_session";
const PLAIN_COOKIE: &str = "mini8n_session";

/// La requête est-elle arrivée en HTTPS (selon la politique configurée) ?
fn is_secure(state: &AppState, headers: &HeaderMap) -> bool {
    match state.config.cookie_secure {
        CookieSecure::Always => true,
        CookieSecure::Never => false,
        CookieSecure::Auto => {
            state.config.trust_proxy
                && headers
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.split(',').next())
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case("https"))
        }
    }
}

fn session_cookie(
    state: &AppState,
    headers: &HeaderMap,
    value: &str,
    max_age: i64,
) -> AppResult<HeaderValue> {
    // HTTPS : préfixe `__Host-` (Secure, sans Domain, Path=/ imposés par le navigateur).
    let (name, secure) = if is_secure(state, headers) {
        (SECURE_COOKIE, "; Secure")
    } else {
        (PLAIN_COOKIE, "")
    };
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Strict{secure}"
    ))
    .map_err(AppError::internal)
}

/// Jetons de session présents (le cookie `__Host-` d'abord). Plusieurs peuvent coexister,
/// par exemple un ancien cookie HTTPS périmé à côté d'un cookie HTTP valide : chacun est essayé.
fn read_cookies(headers: &HeaderMap) -> Vec<String> {
    let pairs: Vec<(String, String)> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    [SECURE_COOKIE, PLAIN_COOKIE]
        .iter()
        .flat_map(|name| {
            pairs
                .iter()
                .filter(move |(k, _)| k == name)
                .map(|(_, v)| v.clone())
        })
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .take(4)
        .collect()
}

/// Efface les deux variantes du cookie de session.
fn clear_cookies() -> [HeaderValue; 2] {
    [
        HeaderValue::from_static(
            "__Host-mini8n_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict; Secure",
        ),
        HeaderValue::from_static("mini8n_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"),
    ]
}

fn logged_out() -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    for c in clear_cookies() {
        res.headers_mut().append(header::SET_COOKIE, c);
    }
    res
}

async fn create_session(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Uuid,
    ip: &str,
) -> AppResult<HeaderValue> {
    let ua = user_agent(headers);
    let token = new_token()?;
    let ttl = Duration::hours(state.config.session_ttl_hours);
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND expires_at <= now()")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    // Au-delà de la limite, les sessions les plus anciennes sont révoquées.
    sqlx::query(
        "DELETE FROM sessions WHERE token_hash IN (
            SELECT token_hash FROM sessions WHERE user_id = $1
            ORDER BY last_seen_at DESC OFFSET $2)",
    )
    .bind(user_id)
    .bind(MAX_SESSIONS_PER_USER - 1)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at, ip, user_agent) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(token_hash(&token))
    .bind(user_id)
    .bind(Utc::now() + ttl)
    .bind(ip)
    .bind(ua)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    session_cookie(state, headers, &token, ttl.num_seconds())
}

/// Utilisateur authentifié (extracteur : 401 si absent).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    session_hash: Vec<u8>,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let mut found = None;
        for token in read_cookies(&parts.headers) {
            let hash = token_hash(&token);
            let row: Option<(Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(
                "SELECT u.id, u.email, u.display_name, s.last_seen_at
                 FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = $1 AND s.expires_at > now()",
            )
            .bind(&hash)
            .fetch_optional(&state.db)
            .await?;
            if let Some(row) = row {
                found = Some((row, hash));
                break;
            }
        }
        let ((id, email, name, last_seen), hash) = found.ok_or(AppError::Unauthorized)?;
        if Utc::now() - last_seen > Duration::minutes(5) {
            sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE token_hash = $1")
                .bind(&hash)
                .execute(&state.db)
                .await?;
        }
        Ok(AuthUser {
            id,
            email,
            name,
            session_hash: hash,
        })
    }
}

impl AuthUser {
    /// Hash du jeton de la session qui a authentifié la requête.
    pub(crate) fn session_hash(&self) -> &[u8] {
        &self.session_hash
    }

    #[cfg(test)]
    pub(crate) fn for_tests(id: Uuid, name: &str) -> Self {
        Self {
            id,
            email: format!("{name}@exemple.test"),
            name: name.into(),
            session_hash: vec![],
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct UserDto {
    id: Uuid,
    email: String,
    name: String,
}

#[derive(Serialize)]
pub struct MeResponse {
    user: UserDto,
    workspaces: Vec<WorkspaceSummary>,
    #[serde(rename = "allowRegistration")]
    allow_registration: bool,
}

async fn me_response(
    state: &AppState,
    id: Uuid,
    email: String,
    name: String,
) -> AppResult<MeResponse> {
    Ok(MeResponse {
        user: UserDto { id, email, name },
        workspaces: list_for_user(&state.db, id).await?,
        allow_registration: state.config.allow_registration,
    })
}

fn with_cookie(status: StatusCode, cookie: HeaderValue, body: impl IntoResponse) -> Response {
    let mut res = (status, body).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie);
    res
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterBody {
    email: String,
    name: String,
    password: String,
}

pub async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(body): Json<RegisterBody>,
) -> AppResult<Response> {
    if !state.config.allow_registration {
        return Err(AppError::Forbidden("Les inscriptions sont fermées".into()));
    }
    state.limits.register_ip.limit(&ip)?;
    let (email, email_normalized) = validate::email(&body.email)?;
    let name = validate::label(&body.name, "Nom", 80)?;
    validate::password(&body.password, &email_normalized)?;

    let hash = hash_password(&state, body.password).await?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO users (id, email, email_normalized, display_name, password_hash)
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (email_normalized) DO NOTHING",
    )
    .bind(id)
    .bind(&email)
    .bind(&email_normalized)
    .bind(&name)
    .bind(&hash)
    .execute(&state.db)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(AppError::Conflict(
            "Impossible de créer ce compte. Si vous en avez déjà un, connectez-vous.".into(),
        ));
    }
    tracing::info!(user_id = %id, "compte créé");
    let cookie = create_session(&state, &headers, id, &ip).await?;
    let me = me_response(&state, id, email, name).await?;
    Ok(with_cookie(StatusCode::CREATED, cookie, Json(me)))
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    id: Uuid,
    email: String,
    display_name: String,
    password_hash: String,
    failed_logins: i32,
    locked_until: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginBody {
    email: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> AppResult<Response> {
    state.limits.login_ip.limit(&ip)?;
    let normalized = body.email.trim().to_lowercase();
    if normalized.len() > 254 || body.password.chars().count() > validate::PASSWORD_MAX {
        return Err(AppError::InvalidCredentials);
    }
    state.limits.login_account.limit(&normalized)?;

    let user: Option<LoginRow> = sqlx::query_as(
        "SELECT id, email, display_name, password_hash, failed_logins, locked_until
         FROM users WHERE email_normalized = $1",
    )
    .bind(&normalized)
    .fetch_optional(&state.db)
    .await?;

    let hash = user.as_ref().map(|u| u.password_hash.clone());
    let valid = verify_password(&state, body.password, hash).await?;
    let Some(LoginRow {
        id,
        email,
        display_name: name,
        failed_logins: failures,
        locked_until,
        ..
    }) = user
    else {
        return Err(AppError::InvalidCredentials);
    };
    if let Some(until) = locked_until
        && until > Utc::now()
    {
        return Err(AppError::TooManyRequests(
            (until - Utc::now()).num_seconds().max(1) as u64,
        ));
    }
    if !valid {
        let failures = failures + 1;
        // Blocage progressif : 1, 2, 4… minutes (max. 1 h) après 5 échecs.
        let lock = (failures >= LOCK_AFTER_FAILURES).then(|| {
            let minutes = 1i64 << (failures - LOCK_AFTER_FAILURES).min(6);
            Utc::now() + Duration::minutes(minutes.min(60))
        });
        sqlx::query("UPDATE users SET failed_logins = $2, locked_until = $3 WHERE id = $1")
            .bind(id)
            .bind(failures)
            .bind(lock)
            .execute(&state.db)
            .await?;
        tracing::warn!(user_id = %id, %ip, failures, "échec de connexion");
        return Err(AppError::InvalidCredentials);
    }
    sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    let cookie = create_session(&state, &headers, id, &ip).await?;
    let me = me_response(&state, id, email, name).await?;
    Ok(with_cookie(StatusCode::OK, cookie, Json(me)))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    for token in read_cookies(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(token_hash(&token))
            .execute(&state.db)
            .await?;
    }
    Ok(logged_out())
}

/// Configuration publique pour l'écran d'accueil.
pub async fn public_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "allowRegistration": state.config.allow_registration }))
}

pub async fn me(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<MeResponse>> {
    Ok(Json(
        me_response(&state, user.id, user.email, user.name).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PasswordBody {
    current_password: String,
    new_password: String,
}

/// Change le mot de passe et révoque toutes les autres sessions.
pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<PasswordBody>,
) -> AppResult<StatusCode> {
    state
        .limits
        .login_account
        .limit(&user.email.to_lowercase())?;
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    if !verify_password(&state, body.current_password, Some(hash)).await? {
        return Err(AppError::bad("Mot de passe actuel incorrect"));
    }
    validate::password(&body.new_password, &user.email.to_lowercase())?;
    let new_hash = hash_password(&state, body.new_password).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = $2, password_changed_at = now() WHERE id = $1")
        .bind(user.id)
        .bind(new_hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash <> $2")
        .bind(user.id)
        .bind(&user.session_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(user_id = %user.id, "mot de passe modifié");
    Ok(StatusCode::NO_CONTENT)
}

/// Déconnecte toutes les sessions de l'utilisateur.
pub async fn logout_all(State(state): State<AppState>, user: AuthUser) -> AppResult<Response> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(logged_out())
}

/// Purge périodique des sessions expirées.
pub fn spawn_session_cleanup(state: Arc<sqlx::PgPool>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
                .execute(&*state)
                .await
            {
                Ok(r) if r.rows_affected() > 0 => {
                    tracing::info!(n = r.rows_affected(), "sessions expirées purgées")
                }
                Ok(_) => {}
                Err(e) => tracing::error!(%e, "purge des sessions"),
            }
        }
    });
}
