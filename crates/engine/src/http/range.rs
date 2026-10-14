//! Byte ranges over HTTP: the `Range` a request asks for and the
//! `Content-Range` a partial answer carries. HTTP/1.1, HTTP/2, HTTP/3 and the
//! [`Fetcher`](crate::Fetcher) all ask and check through here.

use std::fmt;

use reqwest::header::{CONTENT_RANGE, HeaderMap};

use crate::error::Error;
use crate::http::Validators;
use crate::segment::UNKNOWN_END;

/// The bytes a request asks for: from `from` up to `to` (exclusive), or to the
/// end of the file when `to` is `None`. Never empty: there is no `Range`
/// header for no bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestRange {
    from: u64,
    to: Option<u64>,
}

impl RequestRange {
    /// `[from, to)`, where `to` may be [`UNKNOWN_END`] for "to the end";
    /// `None` when that holds no byte.
    pub fn new(from: u64, to: u64) -> Option<Self> {
        Self::up_to(from, (to != UNKNOWN_END).then_some(to))
    }

    /// `[from, to)`, or everything from `from` when `to` is `None`; `None`
    /// when that holds no byte.
    pub fn up_to(from: u64, to: Option<u64>) -> Option<Self> {
        match to {
            Some(to) if to <= from => None,
            _ => Some(Self { from, to }),
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

/// Parses `bytes 0-99/1000`, `bytes 0-99/*` and `bytes */1000` (RFC 9110,
/// section 14.4): the unit in any case and a space after it, offsets in digits
/// only, a last byte inside the file, and a size the engine can represent.
pub(crate) fn parse_content_range(value: &str) -> Option<ContentRange> {
    let value = value.trim();
    let (unit, rest) = value.split_at_checked(BYTES_UNIT.len())?;
    if !unit.eq_ignore_ascii_case(BYTES_UNIT) {
        return None;
    }
    let rest = rest.strip_prefix(' ')?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(digits(t).filter(|&total| total != UNKNOWN_END)?),
    };
    if span.trim() == "*" {
        return Some(ContentRange { start: None, total });
    }
    let (start, end) = span.split_once('-')?;
    let start = digits(start.trim())?;
    let end_inclusive = digits(end.trim())?;
    let inside = total.is_none_or(|total| end_inclusive < total);
    (start <= end_inclusive && inside).then_some(ContentRange {
        start: Some(start),
        total,
    })
}

/// The range unit, the only one HTTP defines.
const BYTES_UNIT: &str = "bytes";

/// A whole number written in ASCII digits only: `u64::from_str` would also
/// take a leading `+`, which HTTP does not allow.
fn digits(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Checks that a partial answer (`206`) with these headers holds the range
/// asked for of the file the first answer described: its bytes start at
/// `from`, the file is still `expected_total` bytes long when both sizes are
/// known, and its validators do not contradict `expected` (see
/// [`Validators::contradicts`]). The first answer itself passes no size and
/// empty validators. Every partial answer, over HTTP/1.1, HTTP/2, HTTP/3 or
/// from the [`Fetcher`](crate::Fetcher), is checked here.
///
/// # Errors
///
/// [`Error::RangeNotHonored`] for other bytes than asked for, and
/// [`Error::RemoteChanged`] for a file of another size or version.
pub(crate) fn check_partial(
    headers: &HeaderMap,
    from: u64,
    expected_total: Option<u64>,
    expected: &Validators,
) -> Result<ContentRange, Error> {
    let got = content_range(headers);
    let start = got.and_then(|c| c.start);
    if start != Some(from) {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: start,
        });
    }
    let total = got.and_then(|c| c.total);
    check_same_file(headers, total, expected_total, expected)?;
    Ok(ContentRange { start, total })
}

/// Checks that a whole answer (`200`) to a request that continues a transfer,
/// whose first bytes are skipped because they arrived before, is of the same
/// file: the same size (its `Content-Length`, `length`) when both are known,
/// and validators that do not contradict `expected`.
///
/// # Errors
///
/// [`Error::RemoteChanged`] for a file of another size or version.
pub(crate) fn check_whole(
    headers: &HeaderMap,
    length: Option<u64>,
    expected_total: Option<u64>,
    expected: &Validators,
) -> Result<(), Error> {
    check_same_file(headers, length, expected_total, expected)
}

/// [`Error::RemoteChanged`] when an answer saying the file is `total` bytes
/// long, with these headers, is about another file than the one of
/// `expected_total` bytes with `expected` validators.
fn check_same_file(
    headers: &HeaderMap,
    total: Option<u64>,
    expected_total: Option<u64>,
    expected: &Validators,
) -> Result<(), Error> {
    let other_size = matches!((expected_total, total), (Some(known), Some(now)) if known != now);
    if other_size || expected.contradicts(&Validators::from_headers(headers)) {
        return Err(Error::RemoteChanged);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{DATE, ETAG, HeaderValue, LAST_MODIFIED};

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
        let range = |from, to| RequestRange::new(from, to).unwrap().to_string();
        assert_eq!(range(0, 100), "bytes=0-99");
        assert_eq!(range(5, 6), "bytes=5-5");
        assert_eq!(range(5, UNKNOWN_END), "bytes=5-");
        assert_eq!(RequestRange::from(0).to_string(), "bytes=0-");
        assert!(RequestRange::from(0).is_whole());
        assert!(!RequestRange::new(0, 10).unwrap().is_whole());
        // No header asks for no bytes.
        assert_eq!(RequestRange::new(5, 5), None);
        assert_eq!(RequestRange::new(5, 4), None);
        assert_eq!(RequestRange::new(0, 0), None);
        assert_eq!(RequestRange::up_to(3, Some(3)), None);
    }

    #[test]
    fn a_partial_answer_must_be_the_range_asked_for() {
        let none = Validators::default();
        let headers = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_RANGE, HeaderValue::from_static(value));
            headers
        };
        assert!(check_partial(&headers("bytes 10-19/100"), 10, Some(100), &none).is_ok());
        assert!(check_partial(&headers("bytes 10-19/*"), 10, Some(100), &none).is_ok());
        assert!(matches!(
            check_partial(&headers("bytes 0-19/100"), 10, Some(100), &none),
            Err(Error::RangeNotHonored {
                expected: 10,
                got: Some(0)
            })
        ));
        assert!(matches!(
            check_partial(&headers("bytes */100"), 0, None, &none),
            Err(Error::RangeNotHonored { got: None, .. })
        ));
        assert!(matches!(
            check_partial(&HeaderMap::new(), 0, None, &none),
            Err(Error::RangeNotHonored { got: None, .. })
        ));
        assert!(matches!(
            check_partial(&headers("bytes 10-19/101"), 10, Some(100), &none),
            Err(Error::RemoteChanged)
        ));
    }

    /// The headers of an answer; `date` is its `Date`.
    fn answer(
        content_range: Option<&str>,
        etag: Option<&str>,
        modified: Option<&str>,
        date: Option<&str>,
    ) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let mut set = |name, value: Option<&str>| {
            if let Some(value) = value {
                headers.insert(name, HeaderValue::from_str(value).unwrap());
            }
        };
        set(CONTENT_RANGE, content_range);
        set(ETAG, etag);
        set(LAST_MODIFIED, modified);
        set(DATE, date);
        headers
    }

    const MONDAY: &str = "Mon, 05 Oct 2026 10:00:00 GMT";
    const A_SECOND_LATER: &str = "Mon, 05 Oct 2026 10:00:01 GMT";
    const TUESDAY: &str = "Tue, 06 Oct 2026 10:00:00 GMT";
    const WEDNESDAY: &str = "Wed, 07 Oct 2026 10:00:00 GMT";

    #[test]
    fn a_partial_answer_of_another_version_of_the_file_is_refused() {
        let range = Some("bytes 10-19/100");
        let first = answer(None, Some("\"v1\""), Some(MONDAY), Some(WEDNESDAY));
        let known = Validators::from_headers(&first);
        let check = |etag, modified| {
            let later = answer(range, etag, modified, Some(WEDNESDAY));
            check_partial(&later, 10, Some(100), &known)
        };
        assert!(check(Some("\"v1\""), Some(MONDAY)).is_ok());
        // The same tag, marked weak on this answer only.
        assert!(check(Some("W/\"v1\""), Some(MONDAY)).is_ok());
        // A validator left out of an answer proves nothing.
        assert!(check(None, None).is_ok());
        assert!(check(Some("\"v1\""), None).is_ok());
        assert!(check(None, Some(MONDAY)).is_ok());
        // A shared entity tag settles it, whatever the dates say.
        assert!(check(Some("\"v1\""), Some(TUESDAY)).is_ok());
        for (etag, modified) in [
            (Some("\"v2\""), Some(MONDAY)),
            (Some("W/\"v2\""), Some(MONDAY)),
            (None, Some(TUESDAY)),
            (Some("\"v2\""), None),
        ] {
            assert!(
                matches!(check(etag, modified), Err(Error::RemoteChanged)),
                "{etag:?} {modified:?}"
            );
        }
        // Without known validators (the first answer) nothing is compared.
        let first = answer(Some("bytes 0-9/100"), Some("\"v2\""), Some(TUESDAY), None);
        assert!(check_partial(&first, 0, None, &Validators::default()).is_ok());
    }

    /// A `Last-Modified` is only a validator when it is at least a second
    /// older than the answer's `Date` (RFC 9110, section 8.8.2.2): a server
    /// that stamps each answer with the time it was sent says nothing about
    /// the file.
    #[test]
    fn a_last_modified_proves_a_change_only_when_both_are_strong() {
        let range = Some("bytes 10-19/100");
        let check = |first: (&str, Option<&str>), later: (&str, Option<&str>)| {
            let first = answer(None, None, Some(first.0), first.1);
            let later = answer(range, None, Some(later.0), later.1);
            check_partial(&later, 10, Some(100), &Validators::from_headers(&first))
        };
        // Each answer stamped with the time it was sent.
        assert!(
            check(
                (MONDAY, Some(MONDAY)),
                (A_SECOND_LATER, Some(A_SECOND_LATER))
            )
            .is_ok()
        );
        // Without a `Date` there is nothing to tell a strong one by.
        assert!(check((MONDAY, None), (TUESDAY, None)).is_ok());
        // Only one of the two is strong.
        assert!(check((MONDAY, Some(WEDNESDAY)), (TUESDAY, Some(TUESDAY))).is_ok());
        assert!(check((MONDAY, Some(MONDAY)), (TUESDAY, Some(WEDNESDAY))).is_ok());
        // Both strong, and different.
        assert!(matches!(
            check((MONDAY, Some(WEDNESDAY)), (TUESDAY, Some(WEDNESDAY))),
            Err(Error::RemoteChanged)
        ));
        // Both strong, the same time written differently.
        assert!(
            check(
                (MONDAY, Some(WEDNESDAY)),
                ("Monday, 05-Oct-26 10:00:00 GMT", Some(WEDNESDAY))
            )
            .is_ok()
        );
    }

    #[test]
    fn a_whole_answer_that_continues_a_transfer_must_be_the_same_file() {
        let known = Validators::from_headers(&answer(None, Some("\"v1\""), None, None));
        let same = answer(None, Some("\"v1\""), None, None);
        assert!(check_whole(&same, Some(100), Some(100), &known).is_ok());
        assert!(check_whole(&same, None, Some(100), &known).is_ok());
        assert!(check_whole(&same, Some(100), None, &known).is_ok());
        assert!(matches!(
            check_whole(&same, Some(101), Some(100), &known),
            Err(Error::RemoteChanged)
        ));
        let other = answer(None, Some("\"v2\""), None, None);
        assert!(matches!(
            check_whole(&other, Some(100), Some(100), &known),
            Err(Error::RemoteChanged)
        ));
    }

    #[test]
    fn a_content_range_that_cannot_be_right_is_refused() {
        // The last byte must be inside the file.
        assert_eq!(parse_content_range("bytes 0-100/100"), None);
        assert!(parse_content_range("bytes 0-99/100").is_some());
        // u64::MAX stands for an unknown end inside the engine.
        assert_eq!(parse_content_range("bytes 0-1/18446744073709551615"), None);
        assert_eq!(parse_content_range("bytes */18446744073709551615"), None);
        // Digits only.
        assert_eq!(parse_content_range("bytes +1-2/10"), None);
        assert_eq!(parse_content_range("bytes 1-+2/10"), None);
        assert_eq!(parse_content_range("bytes 1-2/+10"), None);
        assert_eq!(parse_content_range("bytes */+10"), None);
        // The unit is case-insensitive and followed by a space.
        assert_eq!(
            parse_content_range("BYTES 1-2/10"),
            Some(ContentRange {
                start: Some(1),
                total: Some(10)
            })
        );
        assert_eq!(parse_content_range("bytes1-2/10"), None);
        assert_eq!(parse_content_range("bytesX 1-2/10"), None);
    }
}
