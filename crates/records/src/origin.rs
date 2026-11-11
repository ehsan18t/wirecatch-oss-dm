use serde::{Deserialize, Serialize};
use wirecatch_protocol::extension::Header;
use wirecatch_protocol::{Secret, UnixMillis};

/// What a download needs from the browser to keep working after a restart:
/// cookies, referrer and other headers, and the user agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub user_agent: Option<String>,
    pub headers: Vec<Header>,
    /// Its cookies and credentials were deleted because it did not finish
    /// within a week, so a failure that needs a fresh link says so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forgot_sign_in: bool,
    /// When a fresh browser capture last replaced the credentials, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_in_refreshed_at: Option<UnixMillis>,
    /// Where the browser download came from, kept separately from request headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_source: Option<Box<BrowserSource>>,
}

/// What the browser saw when it handed the file over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSource {
    pub page: Option<Secret>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub page_redirect_urls: Vec<Secret>,
    pub redirect_urls: Vec<Secret>,
    pub private_window: bool,
    /// The browser's name, before the person or the app chose a different name on disk.
    pub file_name: Option<String>,
}

/// `headers` without the sign-in they carry, or `None` when they carry none: cookies, `Authorization` and any other header that may hold a token are dropped (see [`Header::may_carry_sign_in`]); the referring page and the harmless headers of [`HEADERS_SAFE_FOR_ANY_SITE`](wirecatch_protocol::extension::HEADERS_SAFE_FOR_ANY_SITE) stay.
#[must_use]
pub fn without_sign_in(headers: &[Header]) -> Option<Vec<Header>> {
    let kept: Vec<Header> = headers
        .iter()
        .filter(|h| !h.may_carry_sign_in())
        .cloned()
        .collect();
    (kept.len() != headers.len()).then_some(kept)
}
