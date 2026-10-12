//! Which request headers a client may hand over, and turning them into the engine's
//! header map. Hop-by-hop and framing headers belong to the HTTP layer; `Range`,
//! the conditional headers and `Accept-Encoding` to the engine, which must get the
//! whole file, uncompressed, to split it into byte ranges. Letting a client set
//! them would break or confuse downloads.

use tracing::warn;
use wirecatch_engine::DownloadOptions;
use wirecatch_engine::header::{HeaderMap, HeaderName, HeaderValue};
use wirecatch_product::{PRODUCT_NAME, PRODUCT_VERSION};
use wirecatch_protocol::extension::Header;

/// The engine's request options as the app sends them when no browser handed over its own
/// user agent: the app's name and version (`<name>/<version>`), never the engine library's
/// default, which names the library.
pub(crate) fn download_options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.user_agent = format!("{PRODUCT_NAME}/{PRODUCT_VERSION}");
    options
}

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
    // Conditions could make the server answer 304 or 412 instead of the file.
    "if-match",
    "if-none-match",
    "if-modified-since",
    "if-unmodified-since",
    // The engine asks for the file as is; a compressed body breaks byte ranges.
    "accept-encoding",
    // The engine sets the user agent from its own option.
    "user-agent",
];

/// Headers that must appear at most once in a request.
const SINGLE: &[&str] = &["cookie", "referer", "authorization"];

/// Headers that say nothing about the user, so they may follow a download to
/// another site. Everything else (cookies, credentials, tokens in custom headers,
/// the referring page's address) stays with the site the browser sent it to.
const SAFE_FOR_ANY_SITE: &[&str] = &["accept", "accept-language"];

/// Whether a client may set a header with this name.
pub(crate) fn is_allowed(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    !name.is_empty() && !FORBIDDEN.contains(&name.as_str()) && !name.starts_with("proxy-")
}

/// The `Referer` among `headers`, if any.
pub(crate) fn referrer(headers: &[Header]) -> Option<&str> {
    headers
        .iter()
        .find(|h| h.name.trim().eq_ignore_ascii_case("referer"))
        .map(|h| h.value.as_str())
}

/// `headers` without the sign-in they carry, or `None` when they carry none:
/// cookies, `Authorization` and any other header that may hold a token are
/// dropped; the referring page and the harmless headers of
/// [`SAFE_FOR_ANY_SITE`] stay.
pub(crate) fn without_sign_in(headers: &[Header]) -> Option<Vec<Header>> {
    let kept: Vec<Header> = headers
        .iter()
        .filter(|h| {
            let name = h.name.trim().to_ascii_lowercase();
            name == "referer" || SAFE_FOR_ANY_SITE.contains(&name.as_str())
        })
        .cloned()
        .collect();
    (kept.len() != headers.len()).then_some(kept)
}

/// The headers a download keeps when its link moves to another site (another
/// scheme, host or port).
pub(crate) fn for_another_site(headers: &[Header]) -> Vec<Header> {
    headers
        .iter()
        .filter(|h| SAFE_FOR_ANY_SITE.contains(&h.name.trim().to_ascii_lowercase().as_str()))
        .cloned()
        .collect()
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
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.name.trim().as_bytes()),
            HeaderValue::from_str(header.value.trim()),
        ) {
            if SINGLE.contains(&name.as_str()) && map.contains_key(&name) {
                continue;
            }
            map.append(name, value);
        } else {
            warn!(name = %header.name, "skipping a header that is not valid HTTP");
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent every request of the app starts from, before a browser's own.
    fn app_agent() -> String {
        format!("{PRODUCT_NAME}/{PRODUCT_VERSION}")
    }

    #[test]
    fn requests_name_the_app_not_the_engine() {
        let agent = download_options().user_agent;
        assert_eq!(agent, app_agent());
        assert_ne!(agent, DownloadOptions::default().user_agent);
    }

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
    fn encoding_and_conditional_headers_are_refused() {
        // A browser's Accept-Encoding would turn off the engine's identity default
        // (a compressed body breaks byte ranges), and conditions or a range would
        // make the server answer something other than the whole file.
        for name in [
            "Accept-Encoding",
            "If-None-Match",
            "If-Modified-Since",
            "If-Match",
            "If-Unmodified-Since",
            "If-Range",
            "Range",
        ] {
            assert!(!is_allowed(name), "{name} was allowed");
            let kept = for_capture(None, None, &[header(name, "x")]);
            assert!(kept.is_empty(), "{name} was kept for a capture");
            assert!(
                to_map(&[header(name, "x")]).is_empty(),
                "{name} reached the engine"
            );
        }
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
