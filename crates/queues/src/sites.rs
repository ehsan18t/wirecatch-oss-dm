//! Site claims (D72.10): what a pattern covers, and how specific it is.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};
use wirecatch_protocol::extension::read_site;

const LOCAL: &str = "<local>";
/// Covers only the names under the name that follows.
const UNDER: &str = "*.";
/// Any number in one part of an IPv4 address with trailing `*` parts.
const ANY_PART: &str = "*";
const V4_BITS: u8 = 32;
const V6_BITS: u8 = 128;
const V4_PART_BITS: u8 = 8;

/// One site claim, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SitePattern {
    /// A name and every name under it.
    Name(String),
    /// Only the names under a name.
    Under(String),
    /// Addresses whose first `bits` bits are `network`'s.
    Range { network: IpAddr, bits: u8 },
    /// Any host on the person's own network.
    Local,
}

impl SitePattern {
    /// A site claim as the person wrote it, or `None` when it names no site.
    pub(crate) fn read(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.eq_ignore_ascii_case(LOCAL) {
            return Some(Self::Local);
        }
        if let Some(range) = read_range(input) {
            return Some(range);
        }
        if let Some(name) = input.strip_prefix(UNDER) {
            // `10.*` is an address prefix, read above; `*.10` names nothing.
            return read_name(name).map(Self::Under);
        }
        read_name(input).map(Self::Name)
    }

    /// Whether `url`'s host is covered; `is_local` says whether a host is on the person's own network.
    pub(crate) fn covers(&self, url: &Url, is_local: &dyn Fn(&Url) -> bool) -> bool {
        match (self, url.host()) {
            (Self::Local, Some(_)) => is_local(url),
            (Self::Name(name), Some(Host::Domain(host))) => host == name || is_under(host, name),
            (Self::Under(name), Some(Host::Domain(host))) => is_under(host, name),
            (Self::Range { network, bits }, Some(Host::Ipv4(ip))) => {
                in_range(IpAddr::V4(ip), *network, *bits)
            }
            (Self::Range { network, bits }, Some(Host::Ipv6(ip))) => {
                in_range(IpAddr::V6(ip), *network, *bits)
            }
            _ => false,
        }
    }

    /// How specific the pattern is: a higher number wins. A name and an address never both cover one host, so the two scales only meet at `<local>`, the least specific.
    pub(crate) fn specificity(&self) -> u16 {
        match self {
            Self::Local => 0,
            Self::Under(name) => 2 * labels(name),
            Self::Name(name) => 2 * labels(name) + 1,
            Self::Range { bits, .. } => u16::from(*bits) + 1,
        }
    }
}

/// A host name as the site lists keep it (lower case, international names in ASCII form), when `input` names one and is not an address.
fn read_name(input: &str) -> Option<String> {
    let name = read_site(input).ok().flatten()?;
    let is_address = name.parse::<IpAddr>().is_ok()
        || name
            .strip_prefix('[')
            .and_then(|n| n.strip_suffix(']'))
            .is_some_and(|n| n.parse::<Ipv6Addr>().is_ok());
    (!is_address).then_some(name)
}

/// An address, an address range (`10.0.0.0/8`) or an IPv4 address with trailing `*` parts.
fn read_range(input: &str) -> Option<SitePattern> {
    if let Some((address, bits)) = input.split_once('/') {
        let network = address.parse::<IpAddr>().ok()?;
        let bits = bits.parse::<u8>().ok()?;
        return (bits <= width(network)).then_some(SitePattern::Range { network, bits });
    }
    if let Ok(network) = input
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        return Some(SitePattern::Range {
            bits: width(network),
            network,
        });
    }
    read_v4_prefix(input)
}

/// `10.*` or `192.168.*.*`: whole parts, then only `*`, at least one of each.
fn read_v4_prefix(input: &str) -> Option<SitePattern> {
    let parts: Vec<&str> = input.split('.').collect();
    let given = parts.iter().take_while(|p| **p != ANY_PART).count();
    let rest_wild = parts.get(given..)?.iter().all(|p| *p == ANY_PART);
    if given == 0 || given == parts.len() || parts.len() > 4 || !rest_wild {
        return None;
    }
    let mut octets = [0u8; 4];
    for (slot, part) in octets.iter_mut().zip(&parts[..given]) {
        *slot = part.parse().ok()?;
    }
    let bits = u8::try_from(given).ok()?.checked_mul(V4_PART_BITS)?;
    Some(SitePattern::Range {
        network: IpAddr::V4(Ipv4Addr::from(octets)),
        bits,
    })
}

fn width(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => V4_BITS,
        IpAddr::V6(_) => V6_BITS,
    }
}

/// Whether `ip`'s first `bits` bits are `network`'s.
fn in_range(ip: IpAddr, network: IpAddr, bits: u8) -> bool {
    match (ip, network) {
        (IpAddr::V4(ip), IpAddr::V4(net)) => same_prefix(
            u128::from(ip.to_bits()),
            u128::from(net.to_bits()),
            bits,
            V4_BITS,
        ),
        (IpAddr::V6(ip), IpAddr::V6(net)) => {
            same_prefix(ip.to_bits(), net.to_bits(), bits, V6_BITS)
        }
        _ => false,
    }
}

fn same_prefix(a: u128, b: u128, bits: u8, width: u8) -> bool {
    let ignored = u32::from(width.saturating_sub(bits));
    a.checked_shr(ignored).unwrap_or(0) == b.checked_shr(ignored).unwrap_or(0)
}

/// Whether `host` is a name under `name` (`a.example.com` under `example.com`).
fn is_under(host: &str, name: &str) -> bool {
    host.strip_suffix(name)
        .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.'))
}

fn labels(name: &str) -> u16 {
    u16::try_from(name.split('.').count()).unwrap_or(u16::MAX / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn covers(pattern: &str, link: &str) -> bool {
        SitePattern::read(pattern)
            .unwrap()
            .covers(&url(link), &|u| u.host_str() == Some("nas"))
    }

    #[test]
    fn a_name_covers_itself_and_the_names_under_it() {
        assert!(covers("example.com", "https://example.com/a"));
        assert!(covers("example.com", "https://cdn.example.com/a"));
        assert!(covers(
            "https://Example.COM/page",
            "https://cdn.example.com/a"
        ));
        assert!(!covers("example.com", "https://badexample.com/a"));
        assert!(!covers("example.com", "https://example.com.evil.test/a"));
    }

    #[test]
    fn a_star_pattern_covers_only_the_names_under_it() {
        assert!(covers("*.corp.example", "https://wiki.corp.example/"));
        assert!(!covers("*.corp.example", "https://corp.example/"));
    }

    #[test]
    fn address_patterns_cover_their_ranges() {
        assert!(covers("10.*", "http://10.1.2.3/f"));
        assert!(covers("192.168.*.*", "http://192.168.7.1/f"));
        assert!(!covers("10.*", "http://11.0.0.1/f"));
        assert!(covers("10.0.0.0/8", "http://10.200.0.1/f"));
        assert!(!covers("10.0.0.0/16", "http://10.1.0.1/f"));
        assert!(covers("fd00::/8", "http://[fd12::1]/f"));
        assert!(covers("0.0.0.0/0", "http://8.8.8.8/f"));
        assert!(covers("10.1.2.3", "http://10.1.2.3/f"));
        assert!(!covers("10.*", "http://ten.example/f"));
    }

    #[test]
    fn local_asks_the_network_check() {
        assert!(covers("<local>", "http://nas/f"));
        assert!(covers("<LOCAL>", "http://nas/f"));
        assert!(!covers("<local>", "http://example.com/f"));
    }

    #[test]
    fn patterns_that_name_nothing_are_refused() {
        for bad in [
            "",
            "  ",
            "*.",
            "10.*.1",
            "10.0.0.0/33",
            "fd00::/129",
            "1.2.3.4.*",
            "*",
        ] {
            assert_eq!(SitePattern::read(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_longer_name_is_more_specific_and_local_least() {
        let s = |p: &str| SitePattern::read(p).unwrap().specificity();
        assert!(s("cdn.example.com") > s("example.com"));
        assert!(s("example.com") > s("*.example.com"));
        assert!(s("*.example.com") > s("com"));
        assert!(s("10.0.0.0/16") > s("10.*"));
        assert!(s("10.*") > s("<local>"));
    }
}
