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

    #[test]
    fn a_host_is_on_a_site_it_equals_or_sits_under() {
        assert!(host_on_site("example.com", "example.com"));
        assert!(host_on_site("cdn.example.com", "example.com"));
        assert!(!host_on_site("badexample.com", "example.com"));
        assert!(!host_on_site("example.com.evil.test", "example.com"));
        assert!(!host_on_site("com", "example.com"));
    }

    #[test]
    fn under_needs_a_label_before_the_site() {
        assert!(host_under_site("a.example.com", "example.com"));
        assert!(!host_under_site("example.com", "example.com"));
        assert!(!host_under_site(".example.com", "example.com"));
    }

    #[test]
    fn case_and_trailing_dots_do_not_matter() {
        for (host, site) in [
            ("Example.COM", "example.com"),
            ("example.com.", "example.com"),
            ("example.com", "EXAMPLE.com."),
            ("CDN.example.com.", "example.com"),
        ] {
            assert!(host_on_site(host, site), "{host} on {site}");
        }
        assert!(host_under_site("CDN.Example.com.", "example.com."));
    }

    #[test]
    fn an_address_has_no_names_under_it() {
        assert!(!host_on_site("127.0.0.1", "0.0.1"));
        assert!(!host_under_site("127.0.0.1", "0.0.1"));
        assert!(!host_under_site("a.10.0.0.1", "10.0.0.1"));
        assert!(host_on_site("10.0.0.1", "10.0.0.1"));
        assert!(host_on_site("[::1]", "[::1]"));
    }

    #[test]
    fn an_empty_site_covers_nothing() {
        for site in ["", "."] {
            assert!(!host_on_site("example.com", site), "{site:?}");
            assert!(!host_on_site("example.com.", site), "{site:?}");
            assert!(!host_on_site("", site), "{site:?}");
            assert!(!host_under_site("example.com.", site), "{site:?}");
        }
    }

    #[test]
    fn a_name_split_inside_a_character_is_not_a_match() {
        assert!(!host_on_site("bücher.example", "xcher.example"));
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
