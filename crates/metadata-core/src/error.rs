use thiserror::Error;

pub type Result<T> = std::result::Result<T, MetaError>;

#[derive(Debug, Error)]
pub enum MetaError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("backend '{backend}' unavailable: {reason}")]
    BackendUnavailable { backend: String, reason: String },
    #[error("backend '{backend}' failed: {reason}")]
    BackendFailed { backend: String, reason: String },
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl MetaError {
    pub fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        MetaError::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }
}
