//! Which request headers a client may hand over, and turning them into the engine's
//! header map. Hop-by-hop and framing headers belong to the HTTP layer and `Range`
//! to the engine; letting a client set them would break or confuse downloads.

use tracing::warn;
use wirecatch_engine::header::{HeaderMap, HeaderName, HeaderValue};
use wirecatch_protocol::extension::Header;

/// Headers a client may never set (compared in lower case).
const FORBIDDEN: &[&str] = &[
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
    "te",
    "trailer",
    "keep-alive",
    "range",
    "if-range",
    // The engine sets the user agent from its own option.
    "user-agent",
];

/// Headers that must appear at most once in a request.
const SINGLE: &[&str] = &["cookie", "referer", "authorization"];

/// Whether a client may set a header with this name.
pub(crate) fn is_allowed(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    !name.is_empty() && !FORBIDDEN.contains(&name.as_str()) && !name.starts_with("proxy-")
}

/// The headers to keep for a captured download: the referrer and cookies from
/// their own fields first, then the client's extra headers that are allowed and,
/// for single-valued headers, not already present.
pub(crate) fn for_capture(
    referrer: Option<&str>,
    cookies: Option<&str>,
    extra: &[Header],
) -> Vec<Header> {
    let named = |name: &str, value: Option<&str>| {
        value.map(|value| Header {
            name: name.into(),
            value: value.into(),
        })
    };
    let mut out: Vec<Header> = [named("Referer", referrer), named("Cookie", cookies)]
        .into_iter()
        .flatten()
        .collect();
    for header in extra {
        if !is_allowed(&header.name) {
            warn!(name = %header.name, "ignoring a header the browser may not set");
            continue;
        }
        let single = SINGLE.contains(&header.name.trim().to_ascii_lowercase().as_str());
        if single
            && out
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case(header.name.trim()))
        {
            continue;
        }
        out.push(header.clone());
    }
    out
}

/// The engine's header map. A header that is not valid HTTP, not allowed, or a
/// second copy of a single-valued header is left out rather than failing the
/// download (rows stored by older versions are filtered here too).
pub(crate) fn to_map(headers: &[Header]) -> HeaderMap {
    let mut map = HeaderMap::with_capacity(headers.len());
    for header in headers.iter().filter(|h| is_allowed(&h.name)) {
        match (
            HeaderName::from_bytes(header.name.trim().as_bytes()),
            HeaderValue::from_str(header.value.trim()),
        ) {
            (Ok(name), Ok(value)) => {
                if SINGLE.contains(&name.as_str()) && map.contains_key(&name) {
                    continue;
                }
                map.append(name, value);
            }
            _ => warn!(name = %header.name, "skipping a header that is not valid HTTP"),
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, value: &str) -> Header {
        Header {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn framing_and_hop_by_hop_headers_are_refused() {
        for name in [
            "Host",
            "content-length",
            "Transfer-Encoding",
            "Connection",
            "Upgrade",
            "TE",
            "Trailer",
            "Keep-Alive",
            "Proxy-Authorization",
            "proxy-connection",
            "Range",
            "User-Agent",
            " ",
        ] {
            assert!(!is_allowed(name), "{name} was allowed");
        }
        assert!(is_allowed("Accept-Language"));
        assert!(is_allowed("X-Requested-With"));
    }

    #[test]
    fn cookies_are_never_sent_twice() {
        let headers = for_capture(
            Some("https://example.com/page"),
            Some("session=abc"),
            &[
                header("cookie", "other=1"),
                header("Referer", "https://elsewhere.example"),
                header("Host", "evil.example"),
                header("Accept-Language", "en"),
            ],
        );
        let names: Vec<&str> = headers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["Referer", "Cookie", "Accept-Language"]);

        let map = to_map(&[
            header("Cookie", "a=1"),
            header("Cookie", "b=2"),
            header("Content-Length", "9"),
            header("Bad Name", "x"),
            header("X-Ok", "line\nbreak"),
        ]);
        assert_eq!(map.len(), 1);
        assert_eq!(map["cookie"], "a=1");
    }

    #[test]
    fn extra_cookies_are_kept_when_the_cookie_field_is_empty() {
        let headers = for_capture(None, None, &[header("Cookie", "only=1")]);
        assert_eq!(headers, vec![header("Cookie", "only=1")]);
    }
}
