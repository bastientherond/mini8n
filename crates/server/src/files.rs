//! Fichiers du workspace : créés par le nœud « Créer un fichier » ou téléversés,
//! lus par le nœud « Lire un fichier ». Stockés en base (taille et quota bornés).

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::workspaces::require_member;

pub const MAX_FILE_BYTES: usize = 50 * 1024 * 1024;
const WORKSPACE_QUOTA_BYTES: i64 = 1024 * 1024 * 1024;

/// Type MIME d'après l'extension (liste fermée : rien n'est servi « en ligne »).
pub fn content_type(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("parquet") => "application/vnd.apache.parquet",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("xls") => "application/vnd.ms-excel",
        Some("ods") => "application/vnd.oasis.opendocument.spreadsheet",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Nom de fichier sûr : pas de chemin, caractères simples, 120 caractères max.
pub fn sanitize_name(name: &str) -> Result<String, String> {
    let base = name.trim().rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_string();
    if clean.is_empty() {
        return Err("Nom de fichier invalide".into());
    }
    Ok(clean.chars().take(120).collect())
}

/// Enregistre un fichier (contrôle de taille et de quota).
pub async fn store(
    db: &PgPool,
    ws: Uuid,
    name: &str,
    bytes: &[u8],
    user: Option<Uuid>,
    execution: Option<Uuid>,
) -> Result<FileInfo, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Fichier trop volumineux (50 Mo max.)".into());
    }
    let used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(size), 0)::bigint FROM files WHERE workspace_id = $1",
    )
    .bind(ws)
    .fetch_one(db)
    .await
    .map_err(|e| e.to_string())?;
    if used + bytes.len() as i64 > WORKSPACE_QUOTA_BYTES {
        return Err(
            "Quota de fichiers du workspace atteint (1 Go) : supprimez d'anciens fichiers".into(),
        );
    }
    let name = sanitize_name(name)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO files (id, workspace_id, name, content_type, size, content, created_by, execution_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(ws)
    .bind(&name)
    .bind(content_type(&name))
    .bind(bytes.len() as i64)
    .bind(bytes)
    .bind(user)
    .bind(execution)
    .execute(db)
    .await
    .map_err(|e| e.to_string())?;
    info(db, ws, id).await.map_err(|e| e.to_string())
}

/// Contenu d'un fichier du workspace.
pub async fn read(db: &PgPool, ws: Uuid, id: &str) -> Result<(String, Vec<u8>), String> {
    let id = Uuid::parse_str(id.trim()).map_err(|_| "Choisissez un fichier".to_string())?;
    let row: Option<(String, Vec<u8>)> =
        sqlx::query_as("SELECT name, content FROM files WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(ws)
            .fetch_optional(db)
            .await
            .map_err(|e| e.to_string())?;
    row.ok_or_else(|| "Fichier introuvable dans ce workspace".to_string())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: Uuid,
    pub name: String,
    pub content_type: String,
    pub size: i64,
    pub execution_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

async fn info(db: &PgPool, ws: Uuid, id: Uuid) -> AppResult<FileInfo> {
    sqlx::query_as("SELECT id, name, content_type, size, execution_id, created_at FROM files WHERE id = $1 AND workspace_id = $2")
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
) -> AppResult<Json<Vec<FileInfo>>> {
    require_member(&state.db, ws, user.id).await?;
    Ok(Json(
        sqlx::query_as(
            "SELECT id, name, content_type, size, execution_id, created_at FROM files
             WHERE workspace_id = $1 ORDER BY created_at DESC LIMIT 500",
        )
        .bind(ws)
        .fetch_all(&state.db)
        .await?,
    ))
}

#[derive(Deserialize)]
pub struct UploadQuery {
    name: String,
}

/// Téléversement : corps brut, nom en paramètre (`POST …/files?name=ventes.csv`).
pub async fn upload(
    State(state): State<AppState>,
    user: AuthUser,
    Path(ws): Path<Uuid>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> AppResult<(StatusCode, Json<FileInfo>)> {
    require_member(&state.db, ws, user.id).await?;
    state.limits.writes.limit(&user.id.to_string())?;
    if body.is_empty() {
        return Err(AppError::bad("Fichier vide"));
    }
    let file = store(&state.db, ws, &q.name, &body, Some(user.id), None)
        .await
        .map_err(AppError::BadRequest)?;
    Ok((StatusCode::CREATED, Json(file)))
}

/// Téléchargement : toujours en pièce jointe, sans interprétation par le navigateur.
pub async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<Response> {
    require_member(&state.db, ws, user.id).await?;
    let (name, bytes) = read(&state.db, ws, &id.to_string())
        .await
        .map_err(|_| AppError::NotFound)?;
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let mut res = (StatusCode::OK, bytes).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&name)),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"fichier\"; filename*=UTF-8''{encoded}"
        ))
        .map_err(AppError::internal)?,
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    Ok(res)
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_member(&state.db, ws, user.id).await?;
    let n = sqlx::query("DELETE FROM files WHERE id = $1 AND workspace_id = $2")
        .bind(id)
        .bind(ws)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(sanitize_name("../../etc/passwd").unwrap(), "passwd");
        assert_eq!(
            sanitize_name("C:\\\\temp\\\\rapport final.xlsx").unwrap(),
            "rapport final.xlsx"
        );
        assert_eq!(
            sanitize_name("ventes<script>.csv").unwrap(),
            "ventes_script_.csv"
        );
        assert!(sanitize_name("..").is_err());
        assert_eq!(
            content_type("A.XLSX"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(content_type("page.html"), "application/octet-stream");
    }
}
