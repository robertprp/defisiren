use thiserror::Error;
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("dependency unavailable: {0}")]
    Dependency(String),
    #[error("authorization denied: {0}")]
    Denied(String),
    #[error("durability failure")]
    Durability,
    #[error("provider failure: {0}")]
    Provider(String),
    #[error("RPC rate limited; shared cooldown {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("database operation failed")]
    Database,
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("resource limit: {0}")]
    Limit(String),
}
pub type AppResult<T> = Result<T, Error>;
