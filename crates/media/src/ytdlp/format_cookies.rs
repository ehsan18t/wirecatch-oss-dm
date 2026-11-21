//! Scope the static cookies a yt-dlp format describes before each native HTTP request.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use http::{HeaderName, HeaderValue};
use wirecatch_engine::{HttpCookies, Url};
use wirecatch_protocol::Secret;
use wirecatch_sites::public_suffix::SuffixList;

pub(super) struct FormatCookies(Vec<Cookie>);

impl std::fmt::Debug for FormatCookies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormatCookies")
            .field("count", &self.0.len())
            .finish()
    }
}

struct Cookie {
    pair: Secret,
    origin: String,
    domain: Option<String>,
    path: String,
    secure: bool,
    expires: Option<u64>,
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

impl Cookie {
    fn applies(&self, url: &Url, now: u64) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let path = url.path();
        self.domain
            .as_ref()
            .map_or(host == self.origin, |domain| domain_matches(host, domain))
            && (path == self.path
                || path
                    .strip_prefix(&self.path)
                    .is_some_and(|rest| self.path.ends_with('/') || rest.starts_with('/')))
            && matches!(url.scheme(), "http" | "https")
            && (!self.secure || url.scheme() == "https")
            && self.expires.is_none_or(|expiry| expiry > now)
    }

    fn new(name: &str, value: &str, origin: &str, path: &str) -> Option<Self> {
        HeaderName::from_bytes(name.as_bytes()).ok()?;
        let inner = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(value);
        if !inner.bytes().all(
            |byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e),
        ) {
            return None;
        }
        Some(Self {
            pair: Secret::new(format!("{name}={value}")),
            origin: origin.to_owned(),
            domain: None,
            path: path.to_owned(),
            secure: false,
            expires: None,
        })
    }

    fn domain(&mut self, domain: &str, suffixes: &SuffixList) -> Option<()> {
        let domain = domain.to_ascii_lowercase();
        if let Some(wide) = domain.strip_prefix('.') {
            if !domain_matches(&self.origin, wide)
                || suffixes.registrable_domain(wide).is_none()
                || wide.parse::<std::net::IpAddr>().is_ok()
            {
                return None;
            }
            self.domain = Some(wide.to_owned());
        } else if domain != self.origin {
            return None;
        }
        Some(())
    }
}

impl FormatCookies {
    pub(super) fn parse(raw: &Secret, origin: &Url, suffixes: &SuffixList) -> Option<Self> {
        let host = origin.host_str()?;
        let mut cookies: Vec<Cookie> = Vec::new();
        let mut attributes = HashSet::new();
        for piece in raw.expose().split(';').map(str::trim) {
            let (name, value) = piece.split_once('=').unwrap_or((piece, ""));
            let normalized = name.to_ascii_lowercase();
            let attribute = match normalized.as_str() {
                "domain" | "path" | "secure" | "expires" | "version" => true,
                "max-age" | "samesite" | "httponly" | "partitioned" => return None,
                _ => false,
            };
            if !attribute {
                if !cookies.is_empty() && !complete_scope(&attributes) {
                    return None;
                }
                piece.split_once('=')?;
                attributes.clear();
                cookies.push(Cookie::new(name, value, host, "")?);
                continue;
            }
            if !attributes.insert(normalized.clone()) {
                return None;
            }
            let cookie = cookies.last_mut()?;
            match normalized.as_str() {
                "domain" => cookie.domain(value, suffixes)?,
                "path" if value.starts_with('/') => value.clone_into(&mut cookie.path),
                "secure" if piece.eq_ignore_ascii_case("secure") => cookie.secure = true,
                "expires" => cookie.expires = Some(value.parse().ok()?),
                "version" if value == "0" => {}
                _ => return None,
            }
        }
        complete_scope(&attributes).then_some(Self(cookies))
    }
}

fn complete_scope(attributes: &HashSet<String>) -> bool {
    attributes.contains("domain") && attributes.contains("path")
}

impl HttpCookies for FormatCookies {
    fn header(&self, url: &Url) -> Option<HeaderValue> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        let mut pairs: Vec<_> = self
            .0
            .iter()
            .filter(|cookie| cookie.applies(url, now))
            .collect();
        pairs.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        let joined = pairs
            .iter()
            .map(|cookie| cookie.pair.expose())
            .collect::<Vec<_>>()
            .join("; ");
        if joined.is_empty() {
            return None;
        }
        let mut header = HeaderValue::from_str(&joined).ok()?;
        header.set_sensitive(true);
        Some(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_reevaluate_the_format_cookie_scope() {
        let origin = Url::parse("https://a.example.com/video/clip.mp4").unwrap();
        let raw = Secret::new(
            "local=1; Domain=a.example.com; Path=/video; sibling=2; Domain=.example.com; Path=/video; Secure; expired=3; Domain=.example.com; Path=/video; Expires=1",
        );
        let jar = FormatCookies::parse(&raw, &origin, &SuffixList::built_in()).unwrap();
        for (url, expected) in [
            (origin.as_str(), Some("local=1; sibling=2")),
            ("https://b.example.com/video/clip.mp4", Some("sibling=2")),
            ("http://a.example.com/video/clip.mp4", Some("local=1")),
            ("https://a.example.com/videography", None),
            ("https://example.org/video/clip.mp4", None),
        ] {
            let header = jar.header(&Url::parse(url).unwrap());
            assert_eq!(
                header.as_ref().map(HeaderValue::as_bytes),
                expected.map(str::as_bytes)
            );
        }
        for ambiguous in [
            "bad=1; Path=/",
            "bad=1; Path=/; Domain=.example.com; Domain=a.example.com",
        ] {
            assert!(
                FormatCookies::parse(&Secret::new(ambiguous), &origin, &SuffixList::built_in())
                    .is_none()
            );
        }
        let wide = Secret::new("bad=1; Domain=.com; Path=/");
        assert!(FormatCookies::parse(&wide, &origin, &SuffixList::built_in()).is_none());
    }
}
