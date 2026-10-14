//! What keeps requests away from the local network when they must not reach it.
//!
//! The rule is [`PrivateNetwork`]. A [`PrivateGuard`] carries it out for the
//! requests that start at one URL: a [`Guard`] checked before every request and
//! redirect hop, and a [`FilteringResolver`] every connection resolves names
//! through. The HTTP layer, the [`Fetcher`](crate::Fetcher) and downloads all
//! take them from here.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tracing::debug;
use url::{Host, Url};

use crate::error::{Chain, Error, Result};
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
/// [`DownloadOptions::private_network`](crate::DownloadOptions::private_network). Every
/// address whose [`AddressKind`] is not [`AddressKind::Public`] counts as
/// local: loopback, private, link-local, carrier-grade NAT and the other
/// ranges listed there, and an IPv6 address that only carries one of them.
///
/// # The starting URL
///
/// The URL the requests start at (the fetcher's, or the download's link) is
/// the caller's to vouch for, and the rules treat it so:
///
/// - [`Auto`](Self::Auto) looks its host up once, separately, and turns the
///   guard off when every address it gets is local. Whoever controls the
///   name's answers controls that lookup, so a name chosen by a website must
///   not be started with `Auto`.
/// - [`Block`](Self::Block) allows the starting origin when its host is
///   written as an address, which the caller can classify itself (see
///   [`AddressKind`]). A starting host written as a name gets no exception:
///   it is checked before each request like any other, and every connection
///   to it resolves through the filter, so it cannot reach a local address
///   even if its answer changes between a check and the connection (DNS
///   rebinding).
///
/// # With a proxy
///
/// When a proxy is set in the environment (`HTTP_PROXY`, `HTTPS_PROXY` or
/// `ALL_PROXY`, in upper or lower case), requests go through it, and the
/// proxy, not this computer, looks up each name and connects. The guard then
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
    /// Block when the starting URL is on the internet (the usual case for a
    /// video from a website); allow when it is itself on the local network,
    /// such as a media server at home. Whether it is, is decided from a
    /// lookup of its host when the requests start, so use it only for a URL
    /// the user chose (see the section on the starting URL).
    #[default]
    Auto,
    /// Always block, except the starting origin when its host is written as
    /// an address.
    Block,
    /// Never block.
    Allow,
}

/// What kind of address an IP address is: a public internet host, or one of
/// the ranges [`PrivateNetwork`] keeps requests away from. These are the
/// ranges of the IANA special-purpose address registries that are not
/// reachable on the internet, plus multicast, which is no host to download
/// from.
///
/// An IPv6 address that only carries an IPv4 one is of that IPv4 address's
/// kind: IPv4-mapped (`::ffff:10.0.0.1`), IPv4-compatible (`::10.0.0.1`),
/// NAT64 (`64:ff9b::10.0.0.1`) and 6to4 (`2002:0a00:0001::`) addresses reach
/// or name the IPv4 address inside, so `64:ff9b::192.168.1.1` is
/// [`Private`](Self::Private) and `64:ff9b::8.8.8.8` is
/// [`Public`](Self::Public). Teredo (`2001::/32`) is not unwrapped: the
/// addresses inside it belong to a relay and a router on the internet.
///
/// ```
/// use wirecatch_engine::AddressKind;
///
/// let kind = |address: &str| AddressKind::of(address.parse().unwrap());
/// assert_eq!(kind("100.100.100.100"), AddressKind::SharedAddressSpace);
/// assert_eq!(kind("64:ff9b::c0a8:101"), AddressKind::Private);
/// assert!(kind("93.184.215.14").is_public());
/// ```
///
/// New kinds may be added, and an address that is `Public` now may be given
/// one of them in a later version, as the registries assign new ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AddressKind {
    /// Any other address: a host on the internet.
    Public,
    /// This computer: `127.0.0.0/8` and `::1`.
    Loopback,
    /// No address, or this network: `0.0.0.0/8` and `::`. Connecting to one
    /// reaches this computer on most systems.
    Unspecified,
    /// A private network: `10.0.0.0/8`, `172.16.0.0/12` and `192.168.0.0/16`,
    /// unique local `fc00::/7`, and the old site-local `fec0::/10`.
    Private,
    /// The network link itself: `169.254.0.0/16` (where cloud metadata
    /// services answer) and `fe80::/10`.
    LinkLocal,
    /// Shared address space, `100.64.0.0/10`: carrier-grade NAT, and VPNs
    /// such as Tailscale.
    SharedAddressSpace,
    /// Benchmarking, `198.18.0.0/15` and `2001:2::/48`, used for test
    /// networks.
    Benchmarking,
    /// Documentation: `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`,
    /// `2001:db8::/32` and `3fff::/20`. Examples only, never a real host.
    Documentation,
    /// Multicast: `224.0.0.0/4` and `ff00::/8`.
    Multicast,
    /// Reserved: `240.0.0.0/4` (with the broadcast address
    /// `255.255.255.255`), the IETF protocol assignments `192.0.0.0/24` and
    /// `2001::/23` (without Teredo `2001::/32` and the ranges there that serve
    /// the internet, which are public), the discard-only `100::/64`, the
    /// dummy prefix `100:0:0:1::/64`, and segment routing identifiers
    /// `5f00::/16`.
    Reserved,
    /// Local-use NAT64, `64:ff9b:1::/48`: translated by a gateway on the local
    /// network to wherever it was set up for.
    LocalNat64,
}

impl AddressKind {
    /// The kind of `ip`.
    #[must_use]
    pub fn of(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(v4) => Self::of_v4(v4),
            IpAddr::V6(v6) => embedded_ipv4(v6).map_or_else(|| Self::of_v6(v6), Self::of_v4),
        }
    }

    /// Whether this is a host on the internet, which [`PrivateNetwork`] lets
    /// requests reach; every other kind counts as local.
    #[must_use]
    pub fn is_public(self) -> bool {
        self == Self::Public
    }

    fn of_v4(v4: Ipv4Addr) -> Self {
        match v4.octets() {
            [0, ..] => Self::Unspecified,
            [127, ..] => Self::Loopback,
            [10, ..] | [172, 16..=31, ..] | [192, 168, ..] => Self::Private,
            [169, 254, ..] => Self::LinkLocal,
            [100, 64..=127, ..] => Self::SharedAddressSpace,
            [198, 18..=19, ..] => Self::Benchmarking,
            [192, 0, 2, _] | [198, 51, 100, _] | [203, 0, 113, _] => Self::Documentation,
            [192, 0, 0, _] | [240..=255, ..] => Self::Reserved,
            [224..=239, ..] => Self::Multicast,
            _ => Self::Public,
        }
    }

    /// The kind of an IPv6 address that carries no IPv4 one.
    fn of_v6(v6: Ipv6Addr) -> Self {
        if v6.is_unspecified() {
            return Self::Unspecified;
        }
        if v6.is_loopback() {
            return Self::Loopback;
        }
        match v6.segments() {
            [0xff00..=0xffff, ..] => Self::Multicast,
            [0xfc00..=0xfdff | 0xfec0..=0xfeff, ..] => Self::Private,
            [0xfe80..=0xfebf, ..] => Self::LinkLocal,
            [0x64, 0xff9b, 1, ..] => Self::LocalNat64,
            [0x2001, 0xdb8, ..] | [0x3fff, 0..=0x0fff, ..] => Self::Documentation,
            [0x2001, 2, 0, ..] => Self::Benchmarking,
            // The IETF protocol assignments, `2001::/23`, are not reachable
            // on the internet, except Teredo, the anycast addresses of PCP,
            // TURN and DNS-SD service registration, AMT, AS112 and ORCHIDv2
            // and drone identifiers.
            [0x2001, 0 | 3 | 0x20..=0x3f, ..]
            | [0x2001, 1, 0, 0, 0, 0, 0, 1..=3]
            | [0x2001, 4, 0x112, ..] => Self::Public,
            [0x2001, 0..=0x1ff, ..] | [0x100, 0, 0, 0 | 1, ..] | [0x5f00, ..] => Self::Reserved,
            _ => Self::Public,
        }
    }
}

/// Whether requests must be kept away from `ip`: any kind but public.
pub(crate) fn is_private(ip: IpAddr) -> bool {
    !AddressKind::of(ip).is_public()
}

/// The IPv4 address inside an IPv6 one that is only a way of writing or
/// reaching it: IPv4-mapped `::ffff:a.b.c.d`, IPv4-compatible `::a.b.c.d`,
/// NAT64 `64:ff9b::a.b.c.d` and 6to4 `2002:aabb:ccdd::/48`.
fn embedded_ipv4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    let last = |hi: u16, lo: u16| {
        let [first, second] = hi.to_be_bytes();
        let [third, fourth] = lo.to_be_bytes();
        Ipv4Addr::new(first, second, third, fourth)
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
            debug!(host, error = %Chain(&e), "the name does not resolve");
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
/// when local addresses are allowed. `env` reads the proxy settings of the
/// clients the requests go through.
pub(crate) async fn private_guard(
    url: &Url,
    rule: PrivateNetwork,
    timeout: Duration,
    env: proxy::Env<'_>,
) -> Option<PrivateGuard> {
    if rule == PrivateNetwork::Allow {
        return None;
    }
    // `Auto` trusts the starting host: this lookup, not the connection,
    // decides whether it is on the local network.
    if rule == PrivateNetwork::Auto && all_private(&host_addresses(url, timeout).await) {
        return None;
    }
    Some(guard_from(url, timeout, env))
}

/// The protection for requests starting at `url`, through clients whose proxy
/// settings `env` reads.
pub(crate) fn guard_from(url: &Url, timeout: Duration, env: proxy::Env<'_>) -> PrivateGuard {
    let check = Arc::new(NetworkGuard::new(url, timeout));
    PrivateGuard {
        guard: Guard(Arc::new(move |target: &Url| {
            let (check, target) = (check.clone(), target.clone());
            Box::pin(async move { check.check(&target).await })
        })),
        resolver: Arc::new(FilteringResolver {
            proxies: proxy::host_names(env),
        }),
    }
}

/// Refuses requests to the local network, before they are sent; checked for
/// every request and every redirect hop.
struct NetworkGuard {
    /// The fetcher's own origin, allowed when its host is written as an
    /// address: the caller chose it and could check it. A name is checked
    /// like any other, since what it resolves to can change.
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
        if url.origin() == self.origin && !matches!(url.host(), Some(Host::Domain(_))) {
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
    /// The proxies the user set (see [`proxy::host_names`]), resolved without the
    /// filter: the connection to a proxy is the user's choice, and a request
    /// that names one as its target is still checked by the [`NetworkGuard`].
    proxies: Vec<String>,
}

impl Resolve for FilteringResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let trusted = self.proxies.iter().any(|p| p.eq_ignore_ascii_case(&host));
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), LOOKUP_PORT))
                .await?
                .filter(|a| trusted || !is_private(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!(
                    "{host} points to a device on your own network, which is blocked for safety"
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

    /// Addresses at the edges of each range and inside the IPv6 forms that
    /// carry an IPv4 address, with their kinds.
    const KINDS: &[(&str, AddressKind)] = {
        use AddressKind::*;
        &[
            ("0.0.0.0", Unspecified),
            ("0.255.255.255", Unspecified),
            ("::", Unspecified),
            ("127.0.0.1", Loopback),
            ("127.255.255.255", Loopback),
            ("::1", Loopback),
            ("10.0.0.0", Private),
            ("10.255.255.255", Private),
            ("172.16.0.0", Private),
            ("172.31.255.255", Private),
            ("192.168.0.0", Private),
            ("192.168.255.255", Private),
            ("fc00::", Private),
            ("fd12:3456::1", Private),
            ("fdff:ffff::1", Private),
            ("fec0::1", Private),
            ("feff::1", Private),
            ("169.254.0.0", LinkLocal),
            ("169.254.169.254", LinkLocal),
            ("fe80::1", LinkLocal),
            ("febf:ffff::1", LinkLocal),
            ("100.64.0.0", SharedAddressSpace),
            ("100.100.100.100", SharedAddressSpace),
            ("100.127.255.255", SharedAddressSpace),
            ("198.18.0.0", Benchmarking),
            ("198.19.255.255", Benchmarking),
            ("192.0.2.1", Documentation),
            ("198.51.100.255", Documentation),
            ("203.0.113.0", Documentation),
            ("2001:db8::1", Documentation),
            ("2001:db8:ffff::1", Documentation),
            ("3fff::1", Documentation),
            ("3fff:fff::1", Documentation),
            ("224.0.0.0", Multicast),
            ("239.255.255.255", Multicast),
            ("ff00::", Multicast),
            ("ff02::1", Multicast),
            ("192.0.0.0", Reserved),
            ("192.0.0.170", Reserved),
            ("240.0.0.0", Reserved),
            ("255.255.255.255", Reserved),
            ("100::", Reserved),
            ("100::ffff:ffff:ffff:ffff", Reserved),
            ("100:0:0:1::", Reserved),
            ("100:0:0:1:ffff:ffff:ffff:ffff", Reserved),
            ("2001:1::", Reserved),
            ("2001:1::4", Reserved),
            ("2001:2:1::", Reserved),
            ("2001:4:113::1", Reserved),
            ("2001:10::1", Reserved),
            ("2001:1f:ffff::1", Reserved),
            ("2001:40::1", Reserved),
            ("2001:1ff:ffff::1", Reserved),
            ("5f00::", Reserved),
            ("5f00:ffff::1", Reserved),
            ("2001:2::", Benchmarking),
            ("2001:2:0:ffff::1", Benchmarking),
            ("64:ff9b:1::", LocalNat64),
            ("64:ff9b:1:ffff::1", LocalNat64),
            // An IPv6 address that only carries an IPv4 one is of its kind.
            ("::ffff:10.0.0.1", Private),
            ("::ffff:198.51.100.7", Documentation),
            ("::127.0.0.1", Loopback),
            ("::0.0.0.2", Unspecified),
            ("64:ff9b::a9fe:a9fe", LinkLocal),
            ("64:ff9b::100.64.0.1", SharedAddressSpace),
            ("2002:c0a8:0101::1", Private),
            ("2002:e000:1::", Multicast),
            ("::ffff:127.0.0.1", Loopback),
            ("::10.0.0.1", Private),
            ("64:ff9b::7f00:1", Loopback),
            ("64:ff9b::192.168.1.1", Private),
            ("2002:7f00:1::", Loopback),
            ("2002:a9fe:a9fe:1::5", LinkLocal),
            ("2002:6440:1::", SharedAddressSpace),
            // Local-use NAT64 is not unwrapped: the gateway decides.
            ("64:ff9b:1:abcd::8.8.8.8", LocalNat64),
            // Public, including just outside each range.
            ("8.8.8.8", Public),
            ("2606:4700::1111", Public),
            ("64:ff9b::93.184.215.14", Public),
            ("1.0.0.0", Public),
            ("9.255.255.255", Public),
            ("11.0.0.0", Public),
            ("100.63.255.255", Public),
            ("100.128.0.0", Public),
            ("126.255.255.255", Public),
            ("128.0.0.0", Public),
            ("169.253.255.255", Public),
            ("169.255.0.0", Public),
            ("172.15.255.255", Public),
            ("172.32.0.0", Public),
            ("192.0.1.255", Public),
            ("192.0.3.0", Public),
            ("192.88.99.1", Public),
            ("192.167.255.255", Public),
            ("192.169.0.0", Public),
            ("198.17.255.255", Public),
            ("198.20.0.0", Public),
            ("198.51.99.255", Public),
            ("198.51.101.0", Public),
            ("203.0.112.255", Public),
            ("203.0.114.0", Public),
            ("223.255.255.255", Public),
            ("2001:4860:4860::8888", Public),
            ("2001:db9::1", Public),
            ("2001::1", Public),
            ("3ffe:ffff::1", Public),
            ("3fff:1000::1", Public),
            ("100:0:0:2::", Public),
            ("2001:1::1", Public),
            ("2001:1::3", Public),
            ("2001:3::1", Public),
            ("2001:4:112::1", Public),
            ("2001:20::1", Public),
            ("2001:3f:ffff::1", Public),
            ("2001:200::1", Public),
            ("5eff:ffff::1", Public),
            ("5f01::1", Public),
            // IPv4-translated (SIIT, `::ffff:0:a.b.c.d`) is in neither
            // registry and is left public on purpose.
            ("::ffff:0:a00:1", Public),
            ("fbff::1", Public),
            ("fe00::1", Public),
            ("fe7f::1", Public),
            ("64:ff9b:2::1", Public),
            ("::ffff:8.8.8.8", Public),
            ("::8.8.8.8", Public),
            ("64:ff9b::8.8.8.8", Public),
            ("2002:0808:0808::1", Public),
        ]
    };

    #[test]
    fn each_range_has_its_kind() {
        for &(address, kind) in KINDS {
            let ip: IpAddr = address.parse().unwrap();
            assert_eq!(AddressKind::of(ip), kind, "{address}");
            assert_eq!(kind.is_public(), kind == AddressKind::Public);
            assert_eq!(is_private(ip), !kind.is_public(), "{address}");
        }
    }

    #[tokio::test]
    async fn the_guard_refuses_local_addresses_but_not_its_own_address() {
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
            "http://192.0.2.1/",
            "http://[2001:db8::1]/",
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

        // A starting host written as a name is checked like any other.
        let start = Url::parse("http://localhost:8000/master.m3u8").unwrap();
        let guard = NetworkGuard::new(&start, Duration::from_secs(5));
        let error = guard.check(&start).await.unwrap_err();
        assert!(matches!(error, Error::PrivateNetwork { .. }), "{error}");
    }

    #[tokio::test]
    async fn the_resolver_never_hands_out_local_addresses() {
        use std::str::FromStr;
        let resolver = FilteringResolver {
            proxies: Vec::new(),
        };
        // `localhost` resolves only to loopback addresses, all filtered out.
        let error = match resolver.resolve(Name::from_str("localhost").unwrap()).await {
            Ok(addrs) => panic!("resolved to {:?}", addrs.collect::<Vec<_>>()),
            Err(error) => error,
        };
        assert!(error.to_string().contains("blocked for safety"), "{error}");
    }

    /// A download's host resolves to a local address when it connects, as a
    /// rebinding name server would make it, whatever an earlier lookup by the
    /// caller saw. No connection of the download may reach that address, its
    /// first one included.
    #[tokio::test]
    async fn a_download_cannot_be_rebound_to_the_local_network() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Listen on every address `localhost` names, on one port. A client tries
        // them in turn, and Windows takes about two seconds to refuse a closed
        // loopback port, longer than the stall timeout below; it also lets the
        // check count a connection to any of them.
        let mut port = 0;
        let mut listeners = Vec::new();
        for addr in tokio::net::lookup_host("localhost:0").await.unwrap() {
            match tokio::net::TcpListener::bind((addr.ip(), port)).await {
                Ok(listener) => {
                    port = listener.local_addr().unwrap().port();
                    listeners.push(listener);
                }
                // A host without IPv6 can still list `::1` for `localhost`;
                // nothing connects there either.
                Err(error) if error.kind() == std::io::ErrorKind::AddrNotAvailable => {}
                Err(error) => panic!("{addr}: {error}"),
            }
        }
        assert!(
            !listeners.is_empty(),
            "localhost has no address to listen on"
        );
        let accepted = Arc::new(AtomicUsize::new(0));
        for listener in listeners {
            let counter = accepted.clone();
            tokio::spawn(async move {
                while let Ok((_socket, _)) = listener.accept().await {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
        let url = Url::parse(&format!("http://localhost:{port}/video.mp4")).unwrap();
        let dir = wirecatch_test_support::private_dir("rebind").unwrap();
        let mut request = crate::DownloadRequest::new(url.clone(), dir.path());
        request.options.max_retries = 0;
        request.options.connect_timeout = Duration::from_secs(5);
        request.options.stall_timeout = Duration::from_millis(200);
        // Clients that use no proxy whatever the machine sets, so the address
        // is reached directly unless the guard stops it.
        let clients =
            || crate::http::Clients::with_roots(&request.options, rustls::RootCertStore::empty());
        let run = |guard| {
            crate::download::download_guarded(
                request.clone(),
                Arc::new(crate::Progress::default()),
                CancellationToken::new(),
                clients(),
                guard,
            )
        };
        // Without the guard the download reaches the listener, so the check
        // below proves something.
        assert!(run(None).await.is_err(), "the listener sends nothing");
        assert!(
            accepted.load(Ordering::SeqCst) > 0,
            "the listener was not reached"
        );
        accepted.store(0, Ordering::SeqCst);

        let guard = guard_from(&url, Duration::from_secs(5), clients().environment());
        let result = run(Some(guard)).await;
        assert!(result.is_err(), "the download went through");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            0,
            "a connection reached the local address"
        );
        dir.remove().unwrap();
    }

    #[tokio::test]
    async fn the_proxy_the_user_set_may_be_local() {
        use std::str::FromStr;
        let resolver = FilteringResolver {
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
