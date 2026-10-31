//! Which of the browser's cookies go with a captured request: those the browser itself would
//! send, decided with the app's Public Suffix List, which it keeps current (D44, D112).

use url::{Host, Url};
use wirecatch_protocol::extension::{CookieJar, CookieRequest, SameSite};
use wirecatch_protocol::secret::Secret;

use crate::public_suffix::SuffixList;

/// The `Cookie` header for a request to `url`, from the cookies in `jar`; `None` when none
/// goes. Same-site: every cookie. Cross-site: never `SameSite=Strict`; `Lax` (and cookies
/// without the attribute, which browsers treat as `Lax`) only for a top-level navigation;
/// `SameSite=None` always.
#[must_use]
pub fn cookie_header(jar: &CookieJar, url: &str, suffixes: &SuffixList) -> Option<Secret> {
    let cross_site = is_cross_site(url, &jar.request, suffixes);
    let navigation = jar.request.navigation;
    let header = jar
        .cookies
        .iter()
        .filter(|cookie| match cookie.same_site {
            _ if !cross_site => true,
            SameSite::Strict => false,
            SameSite::Lax | SameSite::Unspecified => navigation,
            SameSite::NoRestriction => true,
        })
        .map(|cookie| {
            let value = cookie.value.expose();
            if cookie.name.is_empty() {
                value.to_owned()
            } else {
                format!("{}={value}", cookie.name)
            }
        })
        .collect::<Vec<_>>()
        .join("; ");
    (!header.is_empty()).then(|| Secret::new(header))
}

/// True when the browser would count the request as cross-site: the page that started it, an
/// address it was redirected from, or the tab's page is on another site (scheme included, as
/// Chromium compares them). An unknown starting page counts as another site, so a page that
/// hides its address gains nothing by it.
#[must_use]
pub fn is_cross_site(url: &str, request: &CookieRequest, suffixes: &SuffixList) -> bool {
    let Some(target) = site_of(url, suffixes) else {
        return true;
    };
    let other = |address: &Option<String>| {
        address
            .as_deref()
            .is_some_and(|address| site_of(address, suffixes).as_ref() != Some(&target))
    };
    request
        .initiator
        .as_deref()
        .and_then(|page| site_of(page, suffixes))
        != Some(target.clone())
        || other(&request.original_url)
        || other(&request.top_level)
}

/// The site of a web address with its scheme (`https://example.com`), or `None` for anything
/// but an http or https address with a host. A host that is an IP address, has an empty label,
/// or is a public suffix itself is its own site.
#[must_use]
pub fn site_of(address: &str, suffixes: &SuffixList) -> Option<String> {
    let url = Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let site = match url.host()? {
        Host::Domain(name) => {
            let name = name.trim_end_matches('.');
            if name.split('.').any(str::is_empty) {
                name.to_owned()
            } else {
                suffixes.registrable_domain(name).unwrap_or(name).to_owned()
            }
        }
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => format!("[{ip}]"),
    };
    Some(format!("{}://{site}", url.scheme()))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use wirecatch_protocol::extension::JarCookie;

    use super::*;

    #[derive(Deserialize)]
    struct Cases {
        sites: Vec<(String, Option<String>)>,
        jar: Vec<SameSite>,
        requests: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        why: String,
        url: String,
        request: CookieRequest,
        sent: Vec<SameSite>,
    }

    fn cases() -> Cases {
        serde_json::from_str(include_str!(
            "../../../packages/protocol/src/cookie-scope.cases.json"
        ))
        .unwrap()
    }

    /// A cookie named after its `SameSite` value, as the cases name them.
    fn cookie(same_site: SameSite) -> JarCookie {
        let name = serde_json::to_value(same_site).unwrap();
        JarCookie {
            name: name.as_str().unwrap().to_owned(),
            value: Secret::new("1"),
            same_site,
        }
    }

    #[test]
    fn a_site_is_its_scheme_and_registrable_domain() {
        let suffixes = SuffixList::built_in();
        for (address, site) in cases().sites {
            assert_eq!(site_of(&address, &suffixes), site, "{address}");
        }
    }

    #[test]
    fn a_request_gets_the_cookies_the_browser_would_send() {
        let suffixes = SuffixList::built_in();
        let cases = cases();
        let cookies: Vec<JarCookie> = cases.jar.iter().copied().map(cookie).collect();
        for case in cases.requests {
            let jar = CookieJar {
                cookies: cookies.clone(),
                request: case.request,
            };
            let expected = case
                .sent
                .into_iter()
                .map(|same_site| {
                    let name = cookie(same_site).name;
                    format!("{name}=1")
                })
                .collect::<Vec<_>>()
                .join("; ");
            let header = cookie_header(&jar, &case.url, &suffixes);
            assert_eq!(
                header.as_ref().map(Secret::expose),
                Some(expected.as_str()),
                "{}",
                case.why
            );
        }
    }

    #[test]
    fn no_cookie_gives_no_header() {
        let jar = CookieJar {
            cookies: vec![cookie(SameSite::Strict)],
            request: CookieRequest {
                initiator: None,
                original_url: None,
                top_level: None,
                navigation: true,
            },
        };
        let suffixes = SuffixList::built_in();
        assert_eq!(cookie_header(&jar, "https://example.com/", &suffixes), None);
    }

    #[test]
    fn a_cookie_without_a_name_goes_as_its_value() {
        let jar = CookieJar {
            cookies: vec![
                JarCookie {
                    name: String::new(),
                    value: Secret::new("bare"),
                    same_site: SameSite::Lax,
                },
                cookie(SameSite::Lax),
            ],
            request: CookieRequest {
                initiator: Some("https://example.com/".to_owned()),
                original_url: None,
                top_level: None,
                navigation: true,
            },
        };
        let header = cookie_header(&jar, "https://example.com/f", &SuffixList::built_in());
        assert_eq!(header.as_ref().map(Secret::expose), Some("bare; lax=1"));
    }
}
