//! Page reads made by the browser (D282): the app asks the browser of one connection to make a request of yt-dlp's, so the site sees the browser itself, and the browser answers in parts.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Secret;
use crate::extension::Header;

crate::exported::exported_consts! {
    /// The most bytes of an answer's body in one [`PageReadPart`]: under `MAX_FRAME_LEN` once in base64 and JSON.
    pub const MAX_PAGE_READ_PART: usize = 512 * 1024;
    /// The longest answer body a page read takes; the browser fails a longer one as [`PageReadFailure::TooLarge`].
    pub const MAX_PAGE_READ_LEN: usize = 32 * 1024 * 1024;
    /// The longest request body a page read carries.
    pub const MAX_PAGE_READ_BODY: usize = 256 * 1024;
    /// How long the app waits for the whole answer to a page read, in milliseconds: under yt-dlp's own 20 seconds, so a browser too slow fails the read and the run can read directly.
    pub const PAGE_READ_TIMEOUT_MS: u64 = 15_000;
}

/// What a browser says about page reads in its `hello`: present when it makes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageReads {
    /// The browser's `navigator.userAgent`, which the app matches with a download's to pick the browser it came from.
    pub user_agent: String,
}

/// Identifies one page read, from the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageReadId(#[ts(type = "number & { readonly __brand: \"PageReadId\" }")] pub u64);

/// A request for the browser to make ([`crate::wire::Event::PageReadRequested`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageRead {
    pub id: PageReadId,
    /// `GET`, `HEAD` or `POST`, as yt-dlp sent it.
    pub method: String,
    #[ts(as = "String")]
    pub url: Secret,
    /// yt-dlp's headers; the browser leaves out those it sets itself (`Cookie`, `User-Agent`, `Sec-*`).
    pub headers: Vec<Header>,
    /// The request body in base64, at most [`MAX_PAGE_READ_BODY`] bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
}

/// Says the app no longer waits for a page read ([`crate::wire::Event::PageReadCancelled`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageReadCancelled {
    pub id: PageReadId,
}

/// Part of a page read's answer body, in order ([`crate::wire::Call::PageReadPart`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageReadPart {
    pub id: PageReadId,
    /// At most [`MAX_PAGE_READ_PART`] bytes, in base64.
    pub bytes: String,
}

/// The end of a page read's answer, after its parts ([`crate::wire::Call::PageReadAnswered`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageReadAnswer {
    pub id: PageReadId,
    pub status: u16,
    /// The answer's headers, as the browser shows them to `fetch()`: no `Set-Cookie`.
    pub headers: Vec<Header>,
}

/// Says the browser could not make a page read ([`crate::wire::Call::PageReadFailed`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageReadFailed {
    pub id: PageReadId,
    pub reason: PageReadFailure,
}

/// Why a page read failed in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PageReadFailure {
    /// The request did not reach the site, or its answer did not come back.
    Network,
    /// The answer was longer than [`MAX_PAGE_READ_LEN`].
    TooLarge,
}
