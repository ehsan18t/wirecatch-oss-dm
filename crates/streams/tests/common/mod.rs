//! What the stream tests share: a request with the test settings, and running one to its end.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use url::Url;
use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{Error, Outcome, PrivateNetwork, StreamProgress, StreamRequest, download};

/// The test settings: a short stall timeout, and three retries, enough for a segment a test fails twice.
pub fn options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.stall_timeout = Duration::from_secs(5);
    options.max_retries = 3;
    options
}

/// A request for `url` into `output`, with [`options`].
pub fn request(url: Url, output: impl Into<PathBuf>) -> StreamRequest {
    let mut request = StreamRequest::new(url, output, PrivateNetwork::Auto);
    request.options = options();
    request
}

/// Runs `request` to its end, with no one watching and no pause.
pub async fn run(request: StreamRequest) -> Result<Outcome, Error> {
    download(
        request,
        Arc::new(StreamProgress::default()),
        CancellationToken::new(),
    )
    .await
}
