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
//! matched by the client's own matcher (from `hyper-util`): a list of names and addresses
//! separated by commas, where a name also covers its subdomains (`example.com`
//! and `.example.com` both cover `files.example.com`), an address or a network
//! (`10.0.0.0/8`) covers the addresses in it, and `*` covers every name.

use hyper_util::client::proxy::matcher::Matcher;
use std::ffi::OsString;
use url::{Host, Url};

/// Reads an environment variable; tests pass their own.
pub(crate) type Env<'a> = &'a (dyn Fn(&str) -> Option<OsString> + Sync);

/// Reads the process's environment.
pub(crate) fn system(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// An environment with no variables set.
#[cfg(test)]
pub(crate) fn nothing(_: &str) -> Option<OsString> {
    None
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
    let (variable, proxy) = scheme.variables().into_iter().find_map(|pair| {
        let (name, value) = first_set(&pair, env)?;
        (!value.trim().is_empty()).then_some((name, value))
    })?;
    let no_proxy = first_set(&["NO_PROXY", "no_proxy"], env).map(|(_, value)| value);
    match no_proxy {
        Some(list) if excluded(&proxy, &list, host) => None,
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

/// Whether a request to `host` would bypass the proxy `proxy` because of the
/// `NO_PROXY` list `no_proxy`, decided by the matcher the HTTP client itself
/// uses, so the two never disagree. A host that cannot be put in a URL counts
/// as going through the proxy, the careful answer, and so does a proxy value
/// the matcher cannot read.
fn excluded(proxy: &str, no_proxy: &str, host: &str) -> bool {
    let Ok(destination) = format!("http://{host}/").parse::<::http::Uri>() else {
        return false;
    };
    let intercepts = |no: &str| {
        Matcher::builder()
            .all(proxy.to_string())
            .no(no.to_string())
            .build()
            .intercept(&destination)
            .is_some()
    };
    // A proxy value the matcher cannot read intercepts nothing, with or
    // without NO_PROXY; only a host it would otherwise send through the proxy
    // can be excused by NO_PROXY.
    intercepts("") && !intercepts(no_proxy)
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

    /// A proxy value the client cannot read still counts as a proxy the user
    /// set: an unrelated `NO_PROXY` must not make FTP or QUIC go around it.
    #[test]
    fn an_unreadable_proxy_is_not_excused_by_no_proxy() {
        for proxy in ["not a url at all", "http://[bad"] {
            let set = move |name: &str| match name {
                "ALL_PROXY" => Some(OsString::from(proxy)),
                "NO_PROXY" => Some(OsString::from("other.example")),
                _ => None,
            };
            assert_eq!(
                configured(Scheme::Ftp, "h.example", &set),
                Some("ALL_PROXY"),
                "{proxy}"
            );
            assert_eq!(
                configured(Scheme::Https, "h.example", &set),
                Some("ALL_PROXY"),
                "{proxy}"
            );
        }
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
