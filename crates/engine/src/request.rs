//! What every request of a download needs, handed to the protocol layers
//! (HTTP, HTTP/3, FTP) instead of the download itself.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use url::Url;

/// What every request of one download needs, and all the protocol layers get
/// of it.
pub(crate) struct RequestContext {
    /// Where requests go: the URL after redirects, without credentials.
    pub url: Url,
    /// Whether the server accepts byte ranges.
    pub ranged: bool,
    /// The size of the file, if known.
    pub total: Option<u64>,
    /// Cancelled when the download pauses.
    pub cancel: CancellationToken,
    /// [`DownloadOptions::stall_timeout`](crate::DownloadOptions::stall_timeout).
    pub stall_timeout: Duration,
}
