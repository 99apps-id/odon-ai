use thiserror::Error;

#[derive(Error, Debug)]
pub enum OdonError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Provider error ({provider}): {message}")]
    Provider { provider: String, message: String },

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Security policy violation: {0}")]
    Security(String),

    #[error("Tool execution error ({tool}): {message}")]
    Tool { tool: String, message: String },

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Artifact error: {0}")]
    Artifact(String),

    #[error("Cancelled by user")]
    Cancelled,

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, OdonError>;

/// Child processes and file handles fail with `io::Error`; a `?` on that error
/// should not force every call site to hand-write a conversion.
impl From<std::io::Error> for OdonError {
    fn from(error: std::io::Error) -> Self {
        OdonError::Internal(error.to_string())
    }
}
