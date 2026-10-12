//! Which proxy the environment sets, read the way the HTTP client reads it.
//!
//! The HTTP client takes its proxy from the environment, and some of the
//! engine's own decisions depend on whether a request goes through one:
//! HTTP/3 is not used then (QUIC would go around the proxy), FTP is refused (it
//! cannot go through one yet), and the resolver that keeps downloads off the
//! local network trusts the proxies' own names. They all read the environment
//! here, so they agree with each other and with the client.
//!
//! As in the client, a scheme's own variable (`HTTPS_PROXY` or `https_proxy`
//! for `https`) comes first and `ALL_PROXY` (or `all_proxy`) second, and a host
//! that `NO_PROXY` (or `no_proxy`) names is reached directly. `NO_PROXY` is
//! matched exactly as the client matches it: a list of names and addresses
//! separated by commas, where a name also covers its subdomains (`example.com`
//! and `.example.com` both cover `files.example.com`), an address or a network
//! (`10.0.0.0/8`) covers the addresses in it, and `*` covers every name.

use std::ffi::OsString;
use std::net::IpAddr;

use url::{Host, Url};

/// Reads an environment variable; tests pass their own.
pub(crate) type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// Reads the process's environment.
pub(crate) fn system(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// What a request is sent with, as far as proxies go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Http,
    Https,
    Ftp,
}

impl Scheme {
    /// The variables that set a proxy for this scheme, in the order they are
    /// read: in each pair, the first that is set counts.
    fn variables(self) -> [[&'static str; 2]; 2] {
        let own = match self {
            Scheme::Http => ["HTTP_PROXY", "http_proxy"],
            Scheme::Https => ["HTTPS_PROXY", "https_proxy"],
            Scheme::Ftp => ["FTP_PROXY", "ftp_proxy"],
        };
        [own, ["ALL_PROXY", "all_proxy"]]
    }
}

/// The variable whose proxy a request with `scheme` to `host` goes through, or
/// `None` when it goes directly.
pub(crate) fn configured(scheme: Scheme, host: &str, env: Env<'_>) -> Option<&'static str> {
    let variable = scheme.variables().into_iter().find_map(|pair| {
        let (name, value) = first_set(&pair, env)?;
        (!value.trim().is_empty()).then_some(name)
    })?;
    let no_proxy = first_set(&["NO_PROXY", "no_proxy"], env).map(|(_, value)| value);
    match no_proxy {
        Some(list) if excluded(&list, host) => None,
        _ => Some(variable),
    }
}

/// The host names of every proxy the HTTP client may use (for `http` and
/// `https`). The client connects to these itself, so their names reach the
/// name resolver; a proxy given as an IP address needs no entry.
pub(crate) fn host_names(env: Env<'_>) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for scheme in [Scheme::Http, Scheme::Https] {
        for name in scheme.variables().into_iter().flatten() {
            let Some(value) = env(name) else { continue };
            let value = value.to_string_lossy();
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            // Like the HTTP client, a value without a scheme is an http proxy.
            let parsed = if value.contains("://") {
                Url::parse(value)
            } else {
                Url::parse(&format!("http://{value}"))
            };
            if let Ok(url) = parsed
                && let Some(Host::Domain(host)) = url.host()
                && !hosts.iter().any(|h| h == host)
            {
                hosts.push(host.to_string());
            }
        }
    }
    hosts
}

/// The first of `names` that is set, with its value.
fn first_set(names: &[&'static str], env: Env<'_>) -> Option<(&'static str, String)> {
    names
        .iter()
        .find_map(|&name| env(name).map(|value| (name, value.to_string_lossy().into_owned())))
}

/// Whether the `NO_PROXY` list `list` names `host` (a name, an IP address, or
/// an IPv6 address in brackets).
fn excluded(list: &str, host: &str) -> bool {
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let entries = list.split(',').map(str::trim).filter(|e| !e.is_empty());
    match host.parse::<IpAddr>() {
        Ok(ip) => entries
            .filter_map(network)
            .any(|(net, bits)| within(net, bits, ip)),
        // Entries that are addresses or networks only ever cover addresses.
        Err(_) => entries
            .filter(|entry| network(entry).is_none())
            .any(|entry| names(entry, host)),
    }
}

/// An address (`10.0.0.1`) or network (`10.0.0.0/8`) entry, as the network and
/// the number of leading bits that must match.
fn network(entry: &str) -> Option<(IpAddr, u32)> {
    if let Ok(ip) = entry.parse::<IpAddr>() {
        return Some((ip, full_bits(ip)));
    }
    let (net, bits) = entry.split_once('/')?;
    let net: IpAddr = net.parse().ok()?;
    let bits: u32 = bits.parse().ok()?;
    (bits <= full_bits(net)).then_some((net, bits))
}

/// How many bits an address of this kind has.
fn full_bits(ip: IpAddr) -> u32 {
    match ip {
        IpAddr::V4(_) => u32::BITS,
        IpAddr::V6(_) => u128::BITS,
    }
}

/// Whether `ip` is in the network `net` whose first `bits` bits count. An IPv4
/// address is never in an IPv6 network, nor the other way round.
fn within(net: IpAddr, bits: u32, ip: IpAddr) -> bool {
    match (net, ip) {
        (IpAddr::V4(net), IpAddr::V4(ip)) => {
            let mask = u32::MAX.checked_shl(u32::BITS - bits).unwrap_or(0);
            u32::from(net) & mask == u32::from(ip) & mask
        }
        (IpAddr::V6(net), IpAddr::V6(ip)) => {
            let mask = u128::MAX.checked_shl(u128::BITS - bits).unwrap_or(0);
            u128::from(net) & mask == u128::from(ip) & mask
        }
        _ => false,
    }
}

/// Whether the name entry `entry` covers the host name `host`: the name
/// itself, with or without a leading dot, any subdomain of it, or `*`.
fn names(entry: &str, host: &str) -> bool {
    if entry.eq_ignore_ascii_case(host)
        || entry
            .strip_prefix('.')
            .is_some_and(|e| e.eq_ignore_ascii_case(host))
    {
        return true;
    }
    let Some(cut) = host.len().checked_sub(entry.len()) else {
        return entry == "*";
    };
    let ends_with_entry = host
        .get(cut..)
        .is_some_and(|tail| tail.eq_ignore_ascii_case(entry));
    if ends_with_entry {
        return entry.starts_with('.') || cut > 0 && host.as_bytes().get(cut - 1) == Some(&b'.');
    }
    entry == "*"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An environment holding only `values`.
    fn env(values: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name: &str| {
            values
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn each_scheme_reads_its_own_variable_then_all_proxy() {
        let set = env(&[
            ("https_proxy", "http://proxy:8080"),
            ("ALL_PROXY", "socks5://all:1080"),
        ]);
        assert_eq!(
            configured(Scheme::Https, "h.example", &set),
            Some("https_proxy")
        );
        assert_eq!(
            configured(Scheme::Ftp, "h.example", &set),
            Some("ALL_PROXY")
        );
        assert_eq!(
            configured(Scheme::Http, "h.example", &set),
            Some("ALL_PROXY")
        );
        // The first of a pair that is set counts, even when empty.
        let empty = env(&[("HTTPS_PROXY", ""), ("https_proxy", "http://proxy:8080")]);
        assert_eq!(configured(Scheme::Https, "h.example", &empty), None);
        assert_eq!(configured(Scheme::Ftp, "h.example", &env(&[])), None);
    }

    #[test]
    fn no_proxy_names_hosts_reached_directly() {
        let set = env(&[
            ("ALL_PROXY", "http://proxy:8080"),
            (
                "NO_PROXY",
                "localhost, .internal.example,corp.example,10.0.0.0/8,::1,192.168.1.7,fd00::/8",
            ),
            ("no_proxy", "*"),
        ]);
        for direct in [
            "localhost",
            "LOCALHOST",
            "internal.example",
            "a.internal.example",
            "corp.example",
            "files.corp.example",
            "10.1.2.3",
            "[::1]",
            "192.168.1.7",
            "[fd12::1]",
        ] {
            assert_eq!(configured(Scheme::Ftp, direct, &set), None, "{direct}");
        }
        for proxied in [
            "notcorp.example",
            "example",
            "11.0.0.1",
            "192.168.1.8",
            "[::2]",
            "[fe80::1]",
            "localhost.evil.example",
        ] {
            assert_eq!(
                configured(Scheme::Ftp, proxied, &set),
                Some("ALL_PROXY"),
                "{proxied}"
            );
        }
        // `*` covers every name but no address, as in the HTTP client.
        let star = env(&[("HTTPS_PROXY", "http://proxy:8080"), ("no_proxy", "*")]);
        assert_eq!(configured(Scheme::Https, "anything.example", &star), None);
        assert_eq!(
            configured(Scheme::Https, "8.8.8.8", &star),
            Some("HTTPS_PROXY")
        );
        // An address entry never covers a name that ends like it.
        let address = env(&[("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", "1.2.3.4")]);
        assert_eq!(
            configured(Scheme::Https, "x.1.2.3.4", &address),
            Some("HTTPS_PROXY")
        );
    }

    #[test]
    fn networks_cover_their_addresses_only() {
        let net = |entry: &str| network(entry).unwrap();
        let ip = |text: &str| text.parse::<IpAddr>().unwrap();
        let (v4, bits) = net("100.64.0.0/10");
        assert!(within(v4, bits, ip("100.127.255.255")));
        assert!(!within(v4, bits, ip("100.128.0.0")));
        let (all, bits) = net("0.0.0.0/0");
        assert!(within(all, bits, ip("8.8.8.8")));
        assert!(!within(all, bits, ip("::1")));
        assert_eq!(network("10.0.0.0/33"), None);
        assert_eq!(network("example.com"), None);
    }

    #[test]
    fn proxy_host_names_come_from_the_environment() {
        let hosts = host_names(&env(&[
            ("HTTPS_PROXY", "http://user:secret@Proxy.Corp.example:8080"),
            ("http_proxy", "proxy.corp.example:3128"),
            ("ALL_PROXY", "socks5://socks.lan:1080"),
            ("all_proxy", "http://10.0.0.5:8080"),
            ("HTTP_PROXY", "  "),
            ("https_proxy", "http://[::1]:8080"),
            ("FTP_PROXY", "http://ftp-only.lan:21"),
        ]));
        assert_eq!(hosts, ["proxy.corp.example", "socks.lan"]);
        assert_eq!(
            host_names(&env(&[("NO_PROXY", "localhost")])),
            [] as [String; 0]
        );
    }
}
