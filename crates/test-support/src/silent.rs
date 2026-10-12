//! A server that accepts TCP connections and never answers: what a download
//! waits on while it is connecting, for tests of anything that must happen
//! before the server replies (pausing, removing, queue limits, time-outs).
//!
//! ```no_run
//! # async fn example() {
//! use wirecatch_test_support::silent::SilentServer;
//!
//! let server = SilentServer::start().await;
//! let never = server.url("never.bin");
//! // ... start a download of `never`, then:
//! assert_eq!(server.accepted(), 1);
//! # }
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::net::TcpListener;
use url::Url;

/// A running server; it stops with the test's runtime. Every connection it
/// accepts stays open, unanswered, until then.
pub struct SilentServer {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    accepted: Arc<AtomicUsize>,
}

impl SilentServer {
    /// Listens on a free port of 127.0.0.1.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            // Holding each socket keeps its connection open without a reply.
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                held.push(socket);
            }
        });
        Self {
            base: format!("http://{addr}"),
            accepted,
        }
    }

    /// The URL of `path` on this server.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}/{}", self.base, path)).unwrap()
    }

    /// How many connections the server has accepted so far.
    #[must_use]
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}
