//! Byte ranges over HTTP: the `Range` a request asks for and the
//! `Content-Range` a partial answer carries. HTTP/1.1, HTTP/2, HTTP/3 and the
//! [`Fetcher`](crate::Fetcher) all ask and check through here.

use std::fmt;

use reqwest::header::{CONTENT_RANGE, HeaderMap};

use crate::error::Error;
use crate::segment::UNKNOWN_END;

/// The bytes a request asks for: from `from` up to `to` (exclusive), or to the
/// end of the file when `to` is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestRange {
    pub from: u64,
    pub to: Option<u64>,
}

impl RequestRange {
    /// `[from, to)`, where `to` may be [`UNKNOWN_END`] for "to the end".
    pub fn new(from: u64, to: u64) -> Self {
        Self {
            from,
            to: (to != UNKNOWN_END).then_some(to),
        }
    }

    /// Everything from `from` to the end of the file.
    pub fn from(from: u64) -> Self {
        Self { from, to: None }
    }

    /// Whether this asks for the whole file.
    pub fn is_whole(&self) -> bool {
        self.from == 0 && self.to.is_none()
    }
}

/// The `Range` header value: `bytes=from-last`, with the last byte inclusive,
/// or `bytes=from-` to the end.
impl fmt::Display for RequestRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to {
            Some(to) => write!(f, "bytes={}-{}", self.from, to.saturating_sub(1)),
            None => write!(f, "bytes={}-", self.from),
        }
    }
}

/// What a `Content-Range` header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContentRange {
    /// The offset of the first byte sent; `None` for `bytes */N`, which sends
    /// no bytes of the file and only says its size.
    pub start: Option<u64>,
    /// The size of the whole file, if the server says.
    pub total: Option<u64>,
}

/// The `Content-Range` of an answer, if it has one that can be read.
pub(crate) fn content_range(headers: &HeaderMap) -> Option<ContentRange> {
    parse_content_range(headers.get(CONTENT_RANGE)?.to_str().ok()?)
}

/// Parses `bytes 0-99/1000`, `bytes 0-99/*` and `bytes */1000`.
pub(crate) fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    if span.trim() == "*" {
        return Some(ContentRange { start: None, total });
    }
    let (start, end) = span.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end_inclusive: u64 = end.trim().parse().ok()?;
    (start <= end_inclusive).then_some(ContentRange {
        start: Some(start),
        total,
    })
}

/// Checks that a partial answer (`206`) with these headers holds the range
/// asked for: its bytes start at `from`, and the file is still
/// `expected_total` bytes long when both sizes are known.
///
/// # Errors
///
/// [`Error::RangeNotHonored`] for other bytes than asked for, and
/// [`Error::RemoteChanged`] for a file of another size.
pub(crate) fn check_partial(
    headers: &HeaderMap,
    from: u64,
    expected_total: Option<u64>,
) -> Result<ContentRange, Error> {
    let got = content_range(headers);
    let start = got.and_then(|c| c.start);
    if start != Some(from) {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: start,
        });
    }
    if let (Some(expected), Some(actual)) = (expected_total, got.and_then(|c| c.total))
        && expected != actual
    {
        return Err(Error::RemoteChanged);
    }
    Ok(ContentRange {
        start,
        total: got.and_then(|c| c.total),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn content_range_forms() {
        assert_eq!(
            parse_content_range("bytes 0-99/1000"),
            Some(ContentRange {
                start: Some(0),
                total: Some(1000)
            })
        );
        assert_eq!(
            parse_content_range("bytes */1000"),
            Some(ContentRange {
                start: None,
                total: Some(1000)
            })
        );
        assert_eq!(parse_content_range("bytes 5-9/*").unwrap().total, None);
        assert_eq!(parse_content_range("bytes */0").unwrap().total, Some(0));
        assert_eq!(parse_content_range("bytes 9-5/10"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }

    #[test]
    fn range_headers() {
        assert_eq!(RequestRange::new(0, 100).to_string(), "bytes=0-99");
        assert_eq!(RequestRange::new(5, UNKNOWN_END).to_string(), "bytes=5-");
        assert_eq!(RequestRange::from(0).to_string(), "bytes=0-");
        assert!(RequestRange::from(0).is_whole());
        assert!(!RequestRange::new(0, 10).is_whole());
    }

    #[test]
    fn a_partial_answer_must_be_the_range_asked_for() {
        let headers = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_RANGE, HeaderValue::from_static(value));
            headers
        };
        assert!(check_partial(&headers("bytes 10-19/100"), 10, Some(100)).is_ok());
        assert!(check_partial(&headers("bytes 10-19/*"), 10, Some(100)).is_ok());
        assert!(matches!(
            check_partial(&headers("bytes 0-19/100"), 10, Some(100)),
            Err(Error::RangeNotHonored {
                expected: 10,
                got: Some(0)
            })
        ));
        assert!(matches!(
            check_partial(&headers("bytes */100"), 0, None),
            Err(Error::RangeNotHonored { got: None, .. })
        ));
        assert!(matches!(
            check_partial(&HeaderMap::new(), 0, None),
            Err(Error::RangeNotHonored { got: None, .. })
        ));
        assert!(matches!(
            check_partial(&headers("bytes 10-19/101"), 10, Some(100)),
            Err(Error::RemoteChanged)
        ));
    }
}
