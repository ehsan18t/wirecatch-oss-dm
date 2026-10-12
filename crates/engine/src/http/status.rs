//! What HTTP answers mean to the engine: which ask to come back later, which
//! say the server is busy, and how many redirects are followed. Kept apart so
//! the error type below every layer can use them.

use reqwest::StatusCode;

/// Most redirects followed for one request, the same limit browsers use.
pub(crate) const MAX_REDIRECTS: usize = 20;

/// Whether an answer says "not now" rather than "no", so the request is tried
/// again after a pause: a timeout, too early, busy, or a server error a later
/// attempt often gets past.
pub(crate) fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_EARLY
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

/// Whether an answer says the server is busy: it may say when to come back
/// (`Retry-After`), and to a download already receiving on other connections
/// it means the server allows no more.
pub(crate) fn is_busy(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    )
}
