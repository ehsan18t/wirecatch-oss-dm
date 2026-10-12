//! Helpers the HTTP download tests share.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use wirecatch_engine::units::{KIB, KIB_USIZE};
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, download};
use wirecatch_test_support::http::TestServer;

/// A request for `path` on `server` into `dir` over up to `connections`
/// connections, tuned for small test files: small splits and buffers, so even
/// a few hundred KiB use every connection, a short stall timeout and frequent
/// checkpoints.
pub fn request(server: &TestServer, path: &str, dir: &Path, connections: usize) -> DownloadRequest {
    let mut req = DownloadRequest::new(server.url(path), dir);
    req.options.connections = connections;
    req.options.min_split = 128 * KIB;
    req.options.buffer_size = 64 * KIB_USIZE;
    req.options.stall_timeout = Duration::from_secs(5);
    req.options.checkpoint_interval = Duration::from_millis(100);
    req
}

/// Runs `req` to its end, with no one watching and no pause.
pub async fn run(req: DownloadRequest) -> wirecatch_engine::Result<Outcome> {
    download(req, Arc::new(Progress::default()), CancellationToken::new()).await
}
