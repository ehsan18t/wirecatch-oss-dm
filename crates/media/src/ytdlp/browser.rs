//! Page reads made by the person's browser (D282). Some sites refuse yt-dlp for not being a browser (a TLS reset, a `410`); the browser that sent the download makes yt-dlp's requests to the page's own site instead, so the site sees its TLS, HTTP version, headers and cookies.

use std::future::Future;
use std::net::IpAddr;
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
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The address the browser's connection reached; `None` when the browser did not say.
    pub address: Option<IpAddr>,
}

impl BrowserAnswer {
    /// The browser's answer, when HTTP/1.1 can carry it to yt-dlp: a final status, and header names and values without line breaks or other control characters.
    pub fn checked(
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        address: Option<IpAddr>,
    ) -> Result<Self, BrowserFailed> {
        let fits = (200..=599).contains(&status)
            && headers.iter().all(|(name, value)| {
                http::HeaderName::from_bytes(name.as_bytes()).is_ok()
                    && http::HeaderValue::from_bytes(value.as_bytes()).is_ok()
            });
        if !fits {
            return Err(BrowserFailed::Malformed);
        }
        Ok(Self {
            status,
            headers,
            body,
            address,
        })
    }
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
    #[error("the browser's answer was not one HTTP can carry")]
    Malformed,
    #[error("the browser did not say which address it reached")]
    Unchecked,
}

/// The future of one [`BrowserReader::read`].
pub type BrowserReading =
    Pin<Box<dyn Future<Output = Result<BrowserAnswer, BrowserFailed>> + Send>>;

/// A connected browser that makes requests for the app.
pub trait BrowserReader: Send + Sync {
    /// Makes `request` in the browser. Dropping the future tells the browser to stop.
    fn read(&self, request: BrowserRequest) -> BrowserReading;
}

/// Which hosts of a run go through the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedHosts {
    /// Every host of the page's site (its registrable domain), for a read of the page.
    Site(String),
    /// The page's own host only, for a run that downloads: a video on another host of the site goes directly, never through the browser's memory.
    Page(String),
}

impl RoutedHosts {
    pub(super) fn take(&self, host: &str) -> bool {
        match self {
            RoutedHosts::Site(site) => wirecatch_protocol::hosts::host_on_site(host, site),
            RoutedHosts::Page(page) => host.eq_ignore_ascii_case(page),
        }
    }
}

/// Which browser a run's page reads go through, and for which hosts.
#[derive(Clone)]
pub struct BrowserRoute {
    pub reader: Arc<dyn BrowserReader>,
    pub hosts: RoutedHosts,
}

impl std::fmt::Debug for BrowserRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserRoute")
            .field("hosts", &self.hosts)
            .finish_non_exhaustive()
    }
}
