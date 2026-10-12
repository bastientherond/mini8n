use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Erreurs renvoyées au client. Les détails internes ne sont jamais exposés.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("Authentification requise")]
    Unauthorized,
    #[error("E-mail ou mot de passe incorrect")]
    InvalidCredentials,
    #[error("{0}")]
    Forbidden(String),
    #[error("Ressource introuvable")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("Trop de tentatives, réessayez dans {0} s")]
    TooManyRequests(u64),
    #[error("Erreur interne")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(format!("base de données : {e}"))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized | AppError::InvalidCredentials => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(detail) => {
                tracing::error!(%detail, "erreur interne");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let mut res = (status, Json(json!({ "error": self.to_string() }))).into_response();
        if let AppError::TooManyRequests(secs) = self
            && let Ok(v) = secs.to_string().parse()
        {
            res.headers_mut().insert(axum::http::header::RETRY_AFTER, v);
        }
        res
    }
}
