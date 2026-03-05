use thiserror::Error;

#[derive(Error, Debug)]
pub enum VerySlipError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("DNS error: {0}")]
    Dns(String),

    #[error("QUIC error: {0}")]
    Quic(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Timeout")]
    Timeout,

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Buffer pool exhausted")]
    BufferPoolExhausted,

    #[error("Queue full")]
    QueueFull,

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Metrics error: {0}")]
    Metrics(String),
}

impl From<prometheus::Error> for VerySlipError {
    fn from(err: prometheus::Error) -> Self {
        VerySlipError::Metrics(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, VerySlipError>;
