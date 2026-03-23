use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Query execution failed: {0}")]
    QueryExecution(String),

    #[error("Provider not connected")]
    NotConnected,

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Unsupported operation: {0}")]
    Unsupported(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
}
