//! Whether a host is covered by a site: the one rule behind site lists, queue rules and video detection. Case and trailing dots never matter, and an address has no names under it.

use std::net::IpAddr;

/// Whether `host` is `site` or a name under it.
#[must_use]
pub fn host_on_site(host: &str, site: &str) -> bool {
    let (host, site) = (without_root_dot(host), without_root_dot(site));
    !site.is_empty() && (host.eq_ignore_ascii_case(site) || is_strictly_under(host, site))
}

/// Whether `host` is a name under `site`, with at least one label before it: `a.example.com` under `example.com`, but not `example.com` itself.
#[must_use]
pub fn host_under_site(host: &str, site: &str) -> bool {
    is_strictly_under(without_root_dot(host), without_root_dot(site))
}

/// Whether `host` is an IP address, an IPv6 one with or without its brackets.
#[must_use]
pub fn is_address(host: &str) -> bool {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse::<IpAddr>().is_ok()
}

fn without_root_dot(host: &str) -> &str {
    host.trim_end_matches('.')
}

fn is_strictly_under(host: &str, site: &str) -> bool {
    if site.is_empty() || is_address(host) || is_address(site) {
        return false;
    }
    let Some(split) = host.len().checked_sub(site.len()) else {
        return false;
    };
    let (Some(before), Some(tail)) = (host.get(..split), host.get(split..)) else {
        return false;
    };
    tail.eq_ignore_ascii_case(site)
        && before
            .strip_suffix('.')
            .is_some_and(|label| !label.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Cases {
        on_site: Vec<Case>,
        not_on_site: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        host: String,
        site: String,
    }

    #[test]
    fn hosts_are_on_a_site_as_the_shared_cases_say() {
        let cases: Cases = serde_json::from_str(include_str!(
            "../../../packages/protocol/src/hosts.cases.json"
        ))
        .unwrap();
        for Case { host, site } in cases.on_site {
            assert!(host_on_site(&host, &site), "{host} on {site}");
        }
        for Case { host, site } in cases.not_on_site {
            assert!(!host_on_site(&host, &site), "{host} not on {site}");
        }
    }

    #[test]
    fn under_needs_a_label_before_the_site() {
        assert!(host_under_site("a.example.com", "example.com"));
        assert!(host_under_site("CDN.Example.com.", "example.com."));
        assert!(!host_under_site("example.com", "example.com"));
        assert!(!host_under_site(".example.com", "example.com"));
        assert!(!host_under_site("127.0.0.1", "0.0.1"));
        assert!(!host_under_site("example.com.", "."));
    }

    #[test]
    fn addresses_are_recognised_with_or_without_brackets() {
        for address in ["10.0.0.1", "::1", "[::1]", "[fe80::1]"] {
            assert!(is_address(address), "{address}");
        }
        for name in ["example.com", "[example.com]", "10.0.0", ""] {
            assert!(!is_address(name), "{name}");
        }
    }
}
