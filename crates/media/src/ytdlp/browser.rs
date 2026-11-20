//! Page reads made by the person's browser (D282). Some sites refuse yt-dlp for not being a browser (a TLS reset, a `410`); the browser that sent the download makes yt-dlp's requests to the page's own site instead, so the site sees its TLS, HTTP version, headers and cookies.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use wirecatch_engine::Url;

/// A request of yt-dlp's for the browser to make.
#[derive(Debug, Clone)]
pub struct BrowserRequest {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The browser's answer, its body decoded as the browser decoded it.
#[derive(Debug, Clone)]
pub struct BrowserAnswer {
    pub status: u16,
    /// Where the browser ended up after redirects.
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Why the browser made no answer: the run then reads the page itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BrowserFailed {
    #[error("the browser is no longer connected")]
    Gone,
    #[error("the browser did not answer in time")]
    TimedOut,
    #[error("the browser could not reach the site")]
    Network,
    #[error("the page was too large to read through the browser")]
    TooLarge,
}

/// The future of one [`BrowserReader::read`].
pub type BrowserReading =
    Pin<Box<dyn Future<Output = Result<BrowserAnswer, BrowserFailed>> + Send>>;

/// A connected browser that makes requests for the app.
pub trait BrowserReader: Send + Sync {
    /// Makes `request` in the browser. Dropping the future tells the browser to stop.
    fn read(&self, request: BrowserRequest) -> BrowserReading;
}

/// Which browser a run's page reads go through, and for which site.
#[derive(Clone)]
pub struct BrowserRoute {
    pub reader: Arc<dyn BrowserReader>,
    /// The page's site (its registrable domain): only its hosts go through the browser.
    pub site: String,
}

impl std::fmt::Debug for BrowserRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserRoute")
            .field("site", &self.site)
            .finish_non_exhaustive()
    }
}
