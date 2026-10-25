use serde::{Deserialize, Serialize};
use wirecatch_protocol::extension::Header;

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
