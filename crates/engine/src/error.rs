use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    Msg(String),
    #[error("Arrow : {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("Parquet : {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("JSON : {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Raccourci pour retourner une erreur textuelle.
pub fn fail<T>(msg: impl Into<String>) -> Result<T> {
    Err(EngineError::Msg(msg.into()))
}
