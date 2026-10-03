use thiserror::Error;

/// Result alias for core operations.
pub type CoreResult<T> = Result<T, CoreError>;

/// Errors returned by core database spikes and services.
///
/// Secrets must never be embedded into intentionally constructed messages.
#[derive(Debug, Error, Clone)]
pub enum CoreError {
    #[error("unsupported database kind for this build: {0}")]
    UnsupportedDb(&'static str),

    #[error("connection failed: {0}")]
    Connection(String),

    #[error("query failed: {0}")]
    Query(String),

    #[error("query cancelled")]
    Cancelled,

    #[error("query timed out after {0} ms")]
    Timeout(u64),

    #[error("{0}")]
    Other(String),
}
