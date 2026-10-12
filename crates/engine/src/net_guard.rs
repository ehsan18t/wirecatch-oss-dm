//! What keeps requests away from the local network when they must not reach it.
//!
//! The rule is [`PrivateNetwork`]. A [`PrivateGuard`] carries it out for the
//! requests that start at one URL: a [`Guard`] checked before every request and
//! redirect hop, and a [`FilteringResolver`] every connection resolves names
//! through. The HTTP layer, the [`Fetcher`](crate::Fetcher) and downloads all
//! take them from here.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tracing::debug;
use url::{Host, Url};

use crate::error::{Error, Result};
use crate::proxy;

/// The future a [`Guard`] returns.
pub(crate) type GuardCheck =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>;

/// Decides whether a request may go to an address; an error stops it before
/// anything is sent. Used by [`Fetcher`](crate::Fetcher) to keep streams away
/// from the local network.
#[derive(Clone)]
pub(crate) struct Guard(pub Arc<dyn Fn(&Url) -> GuardCheck + Send + Sync>);

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Guard")
    }
}

/// The port given to name lookups, which only need the name: the addresses a
/// name has do not depend on the port.
const LOOKUP_PORT: u16 = 0;

/// Whether requests may go to the local network: those of a
/// [`Fetcher`](crate::Fetcher), and those of a download with
/// [`DownloadOptions::private_network`](crate::DownloadOptions::private_network). These
/// addresses count as local:
///
/// - loopback (`127.0.0.0/8`, `::1`) and unspecified (`0.0.0.0/8`, `::`);
/// - private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`,
///   and the old site-local `fec0::/10`);
/// - link-local (`169.254.0.0/16`, `fe80::/10`);
/// - shared address space (`100.64.0.0/10`), used by carrier-grade NAT and by
///   VPNs such as Tailscale;
/// - benchmarking (`198.18.0.0/15`), multicast (`224.0.0.0/4`, `ff00::/8`),
///   and reserved and broadcast (`240.0.0.0/4`, so `255.255.255.255` too);
/// - local-use NAT64 (`64:ff9b:1::/48`);
/// - an IPv6 address that only carries an IPv4 one counts as that IPv4
///   address: IPv4-mapped (`::ffff:10.0.0.1`), IPv4-compatible (`::10.0.0.1`),
///   NAT64 (`64:ff9b::10.0.0.1`) and 6to4 (`2002:0a00:0001::`).
///
/// The origin the requests start at (the fetcher's, or the download's link) is
/// always allowed.
///
/// # With a proxy
///
/// When a proxy is set in the environment (`HTTP_PROXY`, `HTTPS_PROXY` or
/// `ALL_PROXY`, in upper or lower case), requests go through it, and the
/// proxy, not Wirecatch, looks up each name and connects. The guard then
/// still refuses, before every request and redirect hop, a target written as a
/// local address and a target whose name resolves only to local addresses on
/// this computer. It cannot see how the proxy resolves a name, so a name that
/// the proxy resolves to a local address (because it changed after the check,
/// or because only the proxy knows it, such as an intranet host) is not
/// caught. The proxy itself is trusted, since the user or their administrator
/// set it, not a website: its own address may be local.
///
/// Requests are not refused outright while a proxy is set, because on
/// networks that need one, every stream would then fail; such a proxy is the
/// network's own gateway and applies its own rules. Use
/// [`PrivateNetwork::Allow`] if those rules should decide alone, or unset the
/// proxy for full protection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrivateNetwork {
    /// Block when the fetcher's own URL is on the internet (the usual case
    /// for a video from a website); allow when it is itself on the local
    /// network, such as a media server at home.
    #[default]
    Auto,
    /// Always block (except the fetcher's own origin).
    Block,
    /// Never block.
    Allow,
}

/// Whether an address is on the local network or otherwise not a public
/// internet host; see [`PrivateNetwork`] for the list. An IPv6 address that
/// carries an IPv4 one (mapped, compatible, NAT64 or 6to4) counts as that
/// IPv4 address.
pub(crate) fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 0 // "this network", 0.0.0.0/8
                || v4.is_loopback() // 127.0.0.0/8
                || v4.is_private() // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local() // 169.254.0.0/16
                || (a == 100 && b & 0xc0 == 64) // shared (CGNAT, Tailscale), 100.64.0.0/10
                || (a == 198 && b & 0xfe == 18) // benchmarking, 198.18.0.0/15
                || v4.is_multicast() // 224.0.0.0/4
                || a >= 240 // reserved and broadcast, 240.0.0.0/4
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = embedded_ipv4(v6) {
                return is_private(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast() // ff00::/8
                || (first & 0xfe00) == 0xfc00 // unique local, fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local, fe80::/10
                || (first & 0xffc0) == 0xfec0 // old site-local, fec0::/10
                // Local-use NAT64, 64:ff9b:1::/48: translated by a gateway on
                // the local network, to wherever it was set up for.
                || v6.segments()[..3] == [0x64, 0xff9b, 1]
        }
    }
}

/// The IPv4 address inside an IPv6 one that is only a way of writing or
/// reaching it: IPv4-mapped `::ffff:a.b.c.d`, IPv4-compatible `::a.b.c.d`,
/// NAT64 `64:ff9b::a.b.c.d` and 6to4 `2002:aabb:ccdd::/48`.
fn embedded_ipv4(v6: std::net::Ipv6Addr) -> Option<std::net::Ipv4Addr> {
    let last = |hi: u16, lo: u16| {
        let [first, second] = hi.to_be_bytes();
        let [third, fourth] = lo.to_be_bytes();
        std::net::Ipv4Addr::new(first, second, third, fourth)
    };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }
    match v6.segments() {
        // `::` and `::1` are themselves, not IPv4 addresses.
        [0, 0, 0, 0, 0, 0, 0, 0 | 1] => None,
        [0, 0, 0, 0, 0, 0, hi, lo] | [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] | [0x2002, hi, lo, ..] => {
            Some(last(hi, lo))
        }
        _ => None,
    }
}

/// Every address `host` resolves to, or nothing if it does not resolve (the
/// request then fails on its own).
async fn resolve(host: &str, timeout: Duration) -> Vec<IpAddr> {
    match tokio::time::timeout(timeout, tokio::net::lookup_host((host, LOOKUP_PORT))).await {
        Ok(Ok(addrs)) => addrs.map(|a| a.ip()).collect(),
        Ok(Err(e)) => {
            debug!(host, error = %e, "the name does not resolve");
            Vec::new()
        }
        Err(_) => {
            debug!(host, "looking up the name timed out");
            Vec::new()
        }
    }
}

/// The addresses of the host of `url`: the address itself, or what the name
/// resolves to (nothing if it does not resolve).
async fn host_addresses(url: &Url, timeout: Duration) -> Vec<IpAddr> {
    match url.host() {
        Some(Host::Ipv4(ip)) => vec![IpAddr::V4(ip)],
        Some(Host::Ipv6(ip)) => vec![IpAddr::V6(ip)],
        Some(Host::Domain(name)) => resolve(name, timeout).await,
        None => Vec::new(),
    }
}

/// Local addresses the starting host may keep using once the guard is on.
/// Only a host that was entirely local (the user deliberately fetching from
/// their own network) keeps them; a host that mixed public and local answers
/// gets none, so a hostile name server cannot slip a local address in beside a
/// public one.
fn pinned_addresses(start: &[IpAddr]) -> Vec<IpAddr> {
    if all_private(start) {
        start.to_vec()
    } else {
        Vec::new()
    }
}

/// Whether a host with these addresses is on the local network: it has
/// addresses and all of them are local. A host that does not resolve counts as
/// public, which keeps the guard on.
fn all_private(addrs: &[IpAddr]) -> bool {
    !addrs.is_empty() && addrs.iter().all(|ip| is_private(*ip))
}

/// What keeps requests that start at one URL away from the local network: a
/// check before every request and redirect hop, and a name resolver that never
/// hands out a local address (so a name that changes its address after the
/// check, DNS rebinding, cannot reach one either). Used by the [`Fetcher`](crate::Fetcher)
/// and by downloads with
/// [`DownloadOptions::private_network`](crate::DownloadOptions::private_network).
pub(crate) struct PrivateGuard {
    pub guard: Guard,
    pub resolver: Arc<FilteringResolver>,
}

/// The protection `rule` asks for, for requests that start at `url`; `None`
/// when local addresses are allowed.
pub(crate) async fn private_guard(
    url: &Url,
    rule: PrivateNetwork,
    timeout: Duration,
) -> Option<PrivateGuard> {
    if rule == PrivateNetwork::Allow {
        return None;
    }
    // The addresses the starting host has right now. Only these may be on the
    // local network later: a name that switches to a local address after this
    // check (DNS rebinding) is filtered like any other.
    let start_addrs = host_addresses(url, timeout).await;
    if rule == PrivateNetwork::Auto && all_private(&start_addrs) {
        return None;
    }
    Some(guard_from(url, &start_addrs, timeout))
}

/// The protection for requests starting at `url`, whose host had `start_addrs`.
pub(crate) fn guard_from(url: &Url, start_addrs: &[IpAddr], timeout: Duration) -> PrivateGuard {
    let check = Arc::new(NetworkGuard::new(url, timeout));
    PrivateGuard {
        guard: Guard(Arc::new(move |target: &Url| {
            let (check, target) = (check.clone(), target.clone());
            Box::pin(async move { check.check(&target).await })
        })),
        resolver: Arc::new(FilteringResolver {
            exempt: url.host_str().map(str::to_string),
            pinned: pinned_addresses(start_addrs),
            proxies: proxy::host_names(&proxy::system),
        }),
    }
}

/// Refuses requests to the local network, before they are sent; checked for
/// every request and every redirect hop.
struct NetworkGuard {
    /// The fetcher's own origin, always allowed.
    origin: url::Origin,
    timeout: Duration,
    /// Names already checked, and whether they were refused.
    names: Mutex<HashMap<String, bool>>,
}

impl NetworkGuard {
    fn new(url: &Url, timeout: Duration) -> Self {
        Self {
            origin: url.origin(),
            timeout,
            names: Mutex::new(HashMap::new()),
        }
    }

    /// Locks the names already checked. A lock poisoned by a panic in another
    /// holder is taken over as it is, for the reason
    /// [`SharedTable::lock`](crate::segment::SharedTable::lock) gives; the cache
    /// holds only finished answers, so it is never half written either.
    fn names(&self) -> MutexGuard<'_, HashMap<String, bool>> {
        self.names.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn check(&self, url: &Url) -> Result<()> {
        if url.origin() == self.origin {
            return Ok(());
        }
        let refused = match url.host() {
            Some(Host::Ipv4(ip)) => is_private(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => is_private(IpAddr::V6(ip)),
            Some(Host::Domain(name)) => {
                let known = self.names().get(name).copied();
                if let Some(refused) = known {
                    refused
                } else {
                    let addrs = resolve(name, self.timeout).await;
                    // Refused when no address outside the local network
                    // is left, the same rule the resolver applies.
                    let refused = !addrs.is_empty() && addrs.into_iter().all(is_private);
                    self.names().insert(name.to_string(), refused);
                    refused
                }
            }
            None => false,
        };
        if refused {
            Err(Error::PrivateNetwork {
                url: url.to_string(),
            })
        } else {
            Ok(())
        }
    }
}

/// Resolves names like the system does, minus local network addresses: the
/// connection itself can then never reach one, even if a name's address
/// changes between the check and the connection.
pub(crate) struct FilteringResolver {
    /// The fetcher's own host.
    exempt: Option<String>,
    /// Local addresses the fetcher's own host had when the fetcher was created;
    /// the only local addresses it may use. Empty when it started public, so a
    /// later switch to a local address is filtered (DNS rebinding).
    pinned: Vec<IpAddr>,
    /// The proxies the user set (see [`proxy::host_names`]), resolved without the
    /// filter: the connection to a proxy is the user's choice, and a request
    /// that names one as its target is still checked by the [`NetworkGuard`].
    proxies: Vec<String>,
}

impl Resolve for FilteringResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let trusted = self.proxies.iter().any(|p| p.eq_ignore_ascii_case(&host));
        let pinned = if self.exempt.as_deref() == Some(host.as_str()) {
            self.pinned.clone()
        } else {
            Vec::new()
        };
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), LOOKUP_PORT))
                .await?
                .filter(|a| trusted || !is_private(a.ip()) || pinned.contains(&a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!(
                    "{host} points to a device on your own network, which Wirecatch blocks for safety"
                )
                .into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn local_network_addresses_are_recognized() {
        for private in [
            "127.0.0.1",
            "127.255.0.9",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.0.1",
            "::ffff:127.0.0.1",
            // Shared address space: carrier-grade NAT, Tailscale.
            "100.64.0.1",
            "100.100.100.100",
            "100.127.255.255",
            // Benchmarking.
            "198.18.0.1",
            "198.19.255.255",
            // Multicast, reserved and broadcast.
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "ff02::1",
            "ff05::1:3",
            // Old site-local IPv6.
            "fec0::1",
            // IPv4 inside IPv6: compatible, NAT64, local-use NAT64, 6to4.
            "::127.0.0.1",
            "::10.0.0.1",
            "::0.0.0.2",
            "64:ff9b::7f00:1",
            "64:ff9b::192.168.1.1",
            "64:ff9b::100.64.0.1",
            "64:ff9b:1::1",
            "64:ff9b:1:abcd::8.8.8.8",
            "2002:7f00:1::",
            "2002:c0a8:0101::1",
            "2002:a9fe:a9fe:1::5",
            "2002:6440:1::",
        ] {
            assert!(is_private(private.parse().unwrap()), "{private}");
        }
        for public in [
            "8.8.8.8",
            "172.32.0.1",
            "192.169.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "198.17.255.255",
            "198.20.0.1",
            "223.255.255.255",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
            "::8.8.8.8",
            "64:ff9b::8.8.8.8",
            "64:ff9b::93.184.215.14",
            "2002:0808:0808::1",
            "fe00::1",
            "2606:4700::1111",
        ] {
            assert!(!is_private(public.parse().unwrap()), "{public}");
        }
    }

    #[tokio::test]
    async fn the_guard_refuses_local_addresses_but_not_its_own_origin() {
        let start = Url::parse("http://127.0.0.1:8000/master.m3u8").unwrap();
        let guard = NetworkGuard::new(&start, Duration::from_secs(5));
        let ok = |u: &str| Url::parse(u).unwrap();
        assert!(
            guard
                .check(&ok("http://127.0.0.1:8000/seg.ts"))
                .await
                .is_ok()
        );
        for refused in [
            "http://127.0.0.1:8001/seg.ts",
            "http://10.0.0.1/seg.ts",
            "http://[::1]:8000/seg.ts",
            "http://169.254.169.254/latest/meta-data",
            "http://[fe80::1]/k",
            "https://192.168.1.1/key",
            "http://localhost:8000/seg.ts",
            "http://100.100.100.100/",
            "http://[64:ff9b::a9fe:a9fe]/latest/meta-data",
            "http://[2002:c0a8:101::1]/admin",
            "http://[::192.168.1.1]/",
            "http://255.255.255.255/",
        ] {
            let error = guard.check(&ok(refused)).await.unwrap_err();
            assert!(matches!(error, Error::PrivateNetwork { .. }), "{refused}");
        }
        assert!(
            guard
                .check(&ok("http://93.184.215.14/seg.ts"))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn the_resolver_never_hands_out_local_addresses() {
        use std::str::FromStr;
        let resolver = FilteringResolver {
            exempt: Some("own-host.invalid".into()),
            pinned: Vec::new(),
            proxies: Vec::new(),
        };
        // `localhost` resolves only to loopback addresses, all filtered out.
        let error = match resolver.resolve(Name::from_str("localhost").unwrap()).await {
            Ok(addrs) => panic!("resolved to {:?}", addrs.collect::<Vec<_>>()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("blocks for safety"), "{error}");
        // The fetcher's own host keeps the local addresses it had at creation.
        let start = host_addresses(
            &Url::parse("http://localhost/").unwrap(),
            Duration::from_secs(5),
        )
        .await;
        assert!(all_private(&start), "localhost resolved to {start:?}");
        let resolver = FilteringResolver {
            exempt: Some("localhost".into()),
            pinned: start.clone(),
            proxies: Vec::new(),
        };
        let addrs: Vec<_> = resolver
            .resolve(Name::from_str("localhost").unwrap())
            .await
            .unwrap()
            .collect();
        assert!(!addrs.is_empty() && addrs.iter().all(|a| start.contains(&a.ip())));
    }

    /// A download's host looked public when it started; it now resolves to a
    /// local address, as a rebinding name server would make it. No connection
    /// of the download may reach that address.
    #[tokio::test]
    async fn a_download_cannot_be_rebound_to_the_local_network() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        tokio::spawn(async move {
            while let Ok((_socket, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        });
        let url = Url::parse(&format!("http://localhost:{port}/video.mp4")).unwrap();
        // The addresses seen at the start: public (none of them local).
        let guard = guard_from(
            &url,
            &["93.184.215.14".parse().unwrap()],
            Duration::from_secs(5),
        );
        let dir = wirecatch_test_support::private_dir("rebind").unwrap();
        let mut request = crate::DownloadRequest::new(url, dir.path());
        request.options.max_retries = 0;
        request.options.connect_timeout = Duration::from_secs(5);
        let clients = crate::http::Clients::new(&request.options).unwrap();
        let result = crate::download::download_guarded(
            request,
            Arc::new(crate::Progress::default()),
            CancellationToken::new(),
            clients,
            Some(guard),
        )
        .await;
        assert!(result.is_err(), "the download went through");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            0,
            "a connection reached the local address"
        );
        dir.remove().unwrap();
    }

    #[test]
    fn a_host_mixing_public_and_local_answers_keeps_no_local_address() {
        let public: IpAddr = "93.184.215.14".parse().unwrap();
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(
            pinned_addresses(&[public, local]),
            [] as [std::net::IpAddr; 0]
        );
        assert_eq!(pinned_addresses(&[public]), [] as [std::net::IpAddr; 0]);
        assert_eq!(pinned_addresses(&[]), [] as [std::net::IpAddr; 0]);
        assert_eq!(pinned_addresses(&[local]), vec![local]);
    }

    #[tokio::test]
    async fn the_own_host_cannot_switch_to_a_local_address_later() {
        use std::str::FromStr;
        // The own host looked public when the fetcher was created (no pinned
        // local addresses); it now resolves to loopback, as a rebinding DNS
        // server would make it. Those addresses must be filtered.
        let resolver = FilteringResolver {
            exempt: Some("localhost".into()),
            pinned: Vec::new(),
            proxies: Vec::new(),
        };
        let error = match resolver.resolve(Name::from_str("localhost").unwrap()).await {
            Ok(addrs) => panic!("rebinding got through: {:?}", addrs.collect::<Vec<_>>()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("blocks for safety"), "{error}");
    }

    #[tokio::test]
    async fn the_proxy_the_user_set_may_be_local() {
        use std::str::FromStr;
        let resolver = FilteringResolver {
            exempt: Some("own-host.invalid".into()),
            pinned: Vec::new(),
            proxies: vec!["localhost".into()],
        };
        let addrs: Vec<_> = resolver
            .resolve(Name::from_str("localhost").unwrap())
            .await
            .unwrap()
            .collect();
        assert!(!addrs.is_empty() && addrs.iter().all(|a| a.ip().is_loopback()));
    }
}
