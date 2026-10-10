use std::io;

/// Errors a download can end with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("server answered {status} for {url}")]
    HttpStatus { status: u16, url: String },

    #[error("server stopped honoring byte ranges (asked for offset {expected}, got {got:?})")]
    RangeNotHonored { expected: u64, got: Option<u64> },

    #[error("the file on the server changed while downloading")]
    RemoteChanged,

    #[error("connection closed early at byte {at} of a part ending at {end}")]
    PrematureEof { at: u64, end: u64 },

    #[error("no data received for {0} seconds")]
    Stalled(u64),

    #[error("disk error: {0}")]
    Io(#[from] io::Error),

    #[error("cancelled before the download started")]
    Cancelled,

    #[error("resume state is unreadable: {0}")]
    State(String),
}

impl Error {
    /// Whether retrying the same request later can reasonably succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Network(_) | Error::PrematureEof { .. } | Error::Stalled(_) => true,
            Error::HttpStatus { status, .. } => {
                matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
