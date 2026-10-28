//! Whether a download's requests may reach the person's local network: who
//! chose its link, and the checks that follow from it (D34.5, D39).

use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use wirecatch_engine::{AddressKind, PrivateNetwork, Url};
use wirecatch_protocol::extension::{CaptureSource, NewDownload, read_site};

use crate::public_suffix::SuffixList;

/// Who chose a download's link. It is set once, where the link arrives, kept
/// with the download, and is the only thing that decides the rule for the
/// local network, so no single call site can pick the wrong one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChosenBy {
    /// The person typed or pasted it into Wirecatch, so it may name a server
    /// on their own network: a NAS or an intranet host.
    Person,
    /// A page on the person's own network: one whose own address says so, or
    /// a site the person listed as on their network (D39). It may lead to
    /// other devices there, as the page itself could.
    LocalPage,
    /// Any other website. Its owner chooses where its names lead, so it may
    /// never lead into the local network.
    Website,
}

impl ChosenBy {
    /// The engine's rule for the local network. [`PrivateNetwork::Block`]
    /// still lets a website's link through when its host is written as an
    /// address (D34.4); [`refuses`] and [`refuses_address`] refuse that case.
    #[must_use]
    pub fn private_network(self) -> PrivateNetwork {
        match self {
            Self::Person | Self::LocalPage => PrivateNetwork::Auto,
            Self::Website => PrivateNetwork::Block,
        }
    }

    /// Who chose a captured link: a local page when every page address the
    /// capture carries (the tab's, the frame's the capture started in, and
    /// the referrer, whichever are present, at least one) is on the person's
    /// network by its own address or is listed in `local_sites`, otherwise a
    /// website. Every one must be: a frame from another site on a local page
    /// chose its own links, and when the extension does not say which frame
    /// started the capture, the referrer or the tab may be the only trace of
    /// it. Nothing here looks a name up: a page's owner controls what its
    /// name answers (CORE-3). Being the same site as the link proves nothing
    /// for the same reason.
    #[must_use]
    pub fn of_capture(
        capture: &NewDownload,
        local_sites: &[String],
        suffixes: &SuffixList,
    ) -> Self {
        let pages = [&capture.page_url, &capture.frame_url, &capture.referrer];
        let mut pages = pages.into_iter().flatten().peekable();
        // Only a browser download's page address is the document's own (its
        // referrer); any other capture may come from a frame, which an older
        // extension names only through the referrer, and that is dropped from
        // https to http. With neither, the frame is unknown, never local.
        let frame_traced = matches!(capture.source, CaptureSource::BrowserDownload)
            || capture.frame_url.is_some()
            || capture.referrer.is_some();
        let carried = frame_traced && pages.peek().is_some();
        let all_local = pages.all(|page| {
            Url::parse(page).is_ok_and(|page| {
                is_local_by_itself(&page, suffixes) || is_listed(&page, local_sites, suffixes)
            })
        });
        if carried && all_local {
            Self::LocalPage
        } else {
            Self::Website
        }
    }
}

/// Names that public DNS does not serve, so a host under one can only be on
/// the person's own network (or in their hosts file), each with its source.
/// The host-naming entries of the IANA Special-Use Domain Names registry are
/// all here except those that name hosts elsewhere: `onion` (RFC 7686) and
/// `alt` (RFC 9476) name hosts on other networks, `ipv4only.arpa` (RFC 8880)
/// resolves publicly, `resolver.arpa` (RFC 9462) names the resolver itself,
/// and the reverse-mapping zones name no hosts.
const LOCAL_SUFFIXES: &[&str] = &[
    // RFC 6761, section 6.3.
    "localhost",
    // RFC 6762 (multicast DNS).
    "local",
    // RFC 6761, sections 6.2, 6.4 and 6.5.
    "test",
    "invalid",
    "example",
    // RFC 8375 (home networks).
    "home.arpa",
    // RFC 9665 (service registration on the local network).
    "service.arpa",
    // Reserved by ICANN for private use (board resolution 2024.07.29.06).
    "internal",
    // ICANN will not delegate them (board resolution 2018.02.04.12).
    "home",
    "corp",
];

/// Names widely used inside networks that nobody has reserved, so ICANN could
/// still delegate them: a product choice, not a standard. Each counts only
/// while the Public Suffix List does not list it, which it would once
/// delegated. (The list's ICANN section is not a test for every name above:
/// it lists `home.arpa`, since each home is its own owner.)
const UNRESERVED_LOCAL_SUFFIXES: &[&str] = &[
    // Home routers' usual domain.
    "lan",
];

/// Whether `url`'s host is on the local network by what is written in it: a
/// local address, a single-label name (`nas`) that is not a top-level domain
/// (public DNS may serve records at `ai.` itself), or a name under one of
/// `LOCAL_SUFFIXES` or `UNRESERVED_LOCAL_SUFFIXES`.
#[must_use]
pub fn is_local_by_itself(url: &Url, suffixes: &SuffixList) -> bool {
    if let Some(ip) = address_in(url) {
        return is_local(ip);
    }
    let Some(name) = url.host_str() else {
        return false;
    };
    let name = name.trim_end_matches('.');
    if name.is_empty() {
        return false;
    }
    if !name.contains('.') {
        return !suffixes.is_public_suffix(name);
    }
    LOCAL_SUFFIXES.iter().any(|suffix| is_under(name, suffix))
        || UNRESERVED_LOCAL_SUFFIXES
            .iter()
            .any(|suffix| is_under(name, suffix) && !suffixes.is_public_suffix(suffix))
}

/// Why an entry cannot be one of the person's sites on their network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotALocalSite {
    /// It names no host.
    Unreadable,
    /// It is an address. A page at a local address is on the network without
    /// being listed, and the names under an address mean nothing.
    Address,
    /// It is a public suffix or above one (`com`, `co.uk`, `github.io`), so
    /// it would make the sites of every owner under it local pages.
    Shared,
}

/// One of the person's sites on their network as it is kept and matched: the
/// host [`read_site`] keeps for what the person typed (a host name or a web
/// address; lower case, international names in ASCII form, no dots at either
/// end, as the URL parser writes a page's host), when it is a registrable
/// domain or a name under one. `None` for an entry of nothing but white space.
pub fn local_site(entry: &str, suffixes: &SuffixList) -> Result<Option<String>, NotALocalSite> {
    let Some(host) = read_site(entry).map_err(|_| NotALocalSite::Unreadable)? else {
        return Ok(None);
    };
    let url = Url::parse(&format!("http://{host}/")).map_err(|_| NotALocalSite::Unreadable)?;
    if address_in(&url).is_some() {
        return Err(NotALocalSite::Address);
    }
    let name = url.domain().ok_or(NotALocalSite::Unreadable)?;
    if suffixes.registrable_domain(name).is_none() {
        return Err(NotALocalSite::Shared);
    }
    Ok(Some(name.to_string()))
}

/// Whether `url`'s host is on the person's own network: by what is written in it, or by being one of `local_sites` or under one.
#[must_use]
pub fn is_on_own_network(url: &Url, local_sites: &[String], suffixes: &SuffixList) -> bool {
    is_local_by_itself(url, suffixes) || is_listed(url, local_sites, suffixes)
}

/// Whether `url`'s host is one of `sites` or under one of them
/// (`corp.example.com` covers `wiki.corp.example.com`). A site counts only
/// while it is one owner's by the list in use, which may be newer than the
/// one it was checked with when it was saved.
fn is_listed(url: &Url, sites: &[String], suffixes: &SuffixList) -> bool {
    let Some(name) = url.host_str() else {
        return false;
    };
    let name = name.trim_end_matches('.');
    sites
        .iter()
        .any(|site| is_under(name, site) && suffixes.registrable_domain(site).is_some())
}

/// Whether host `name` is `domain` or a name under it. Both are lower case:
/// the URL parser and the settings store them so.
fn is_under(name: &str, domain: &str) -> bool {
    name.strip_suffix(domain)
        .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
}

/// How long the check of a captured link waits for its name to resolve. It
/// runs before the browser is answered, well within the extension's wait.
const CAPTURE_CHECK: Duration = Duration::from_secs(3);

/// Whether a link chosen by `chosen_by` is refused before Wirecatch takes it,
/// so the browser keeps its own copy: a website's link whose host is a local
/// address, or a name whose every address is local (the engine's own test for
/// a name under [`PrivateNetwork::Block`]). This lookup can only refuse: a
/// name that answers public here and local when the engine connects is still
/// refused by the engine, and a name that does not answer in time goes on to
/// the engine, which decides.
pub async fn refuses(chosen_by: ChosenBy, url: &Url) -> bool {
    if chosen_by != ChosenBy::Website {
        return false;
    }
    if let Some(ip) = address_in(url) {
        return is_local(ip);
    }
    let found = addresses(url, CAPTURE_CHECK).await;
    !found.is_empty() && found.into_iter().all(is_local)
}

/// [`refuses`] without the lookup, for a download about to start: a website's
/// link written as a local address, which the engine's `Block` lets through as
/// the starting origin (D34.4) because only its caller can classify it.
pub fn refuses_address(chosen_by: ChosenBy, url: &Url) -> bool {
    chosen_by == ChosenBy::Website && address_in(url).is_some_and(is_local)
}

/// The address `url`'s host is written as, if it is one.
#[must_use]
pub fn address_in(url: &Url) -> Option<IpAddr> {
    let host = url.host_str()?;
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .ok()
}

/// The addresses of `url`'s host: the address itself, or what its name
/// resolves to within `timeout` (nothing if it does not).
pub async fn addresses(url: &Url, timeout: Duration) -> Vec<IpAddr> {
    if let Some(ip) = address_in(url) {
        return vec![ip];
    }
    let Some(name) = url.host_str() else {
        return Vec::new();
    };
    let port = url.port_or_known_default().unwrap_or(443);
    match tokio::time::timeout(timeout, tokio::net::lookup_host((name, port))).await {
        Ok(Ok(addrs)) => addrs.map(|a| a.ip()).collect(),
        _ => Vec::new(),
    }
}

/// Whether `ip` is on the local network, by the engine's own table, so the
/// two never disagree (CORE-2).
#[must_use]
pub fn is_local(ip: IpAddr) -> bool {
    !AddressKind::of(ip).is_public()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn a_host_is_local_by_its_own_address_or_name() {
        for local in [
            "http://192.168.1.1/",
            "http://100.100.100.100/",
            "http://[fe80::1]/",
            "http://[64:ff9b::c0a8:101]/",
            "http://nas/",
            "http://nas./",
            "http://printer.local/",
            "http://router.home.arpa/",
            "http://printer.service.arpa/",
            "http://files.corp/",
            "http://wiki.internal/",
            "http://box.lan/",
            "http://app.localhost/",
            "http://LOCALHOST/",
        ] {
            assert!(
                is_local_by_itself(&url(local), &SuffixList::built_in()),
                "{local}"
            );
        }
        // The guard against a delegated name disables none of them today.
        for suffix in UNRESERVED_LOCAL_SUFFIXES {
            assert!(!SuffixList::built_in().is_public_suffix(suffix), "{suffix}");
        }
        for public in [
            "https://93.184.215.14/",
            "https://example.com/",
            "https://local.example.com/",
            "https://evil.example.org/",
            "https://mylocal.com/",
            "http://site.onion/",
            // Top-level domains, which public DNS may serve records at.
            "http://ai./",
            "http://com./",
            "http://xn--p1ai/",
        ] {
            assert!(
                !is_local_by_itself(&url(public), &SuffixList::built_in()),
                "{public}"
            );
        }
    }

    fn capture(page: Option<&str>, referrer: Option<&str>) -> NewDownload {
        NewDownload {
            handover_id: "h".into(),
            url: "http://192.168.1.10/f.zip".into(),
            referrer: referrer.map(str::to_string),
            page_url: page.map(str::to_string),
            frame_url: None,
            suggested_name: None,
            mime: None,
            size_hint: None,
            cookies: None,
            user_agent: None,
            extra_headers: vec![],
            source: CaptureSource::BrowserDownload,
            queue: None,
            media: None,
        }
    }

    #[test]
    fn a_capture_is_a_local_pages_only_by_the_pages_own_address_or_the_list() {
        let none: &[String] = &[];
        let listed = ["corp.example.com".to_string()];
        let of = |page, referrer, sites: &[String]| {
            ChosenBy::of_capture(&capture(page, referrer), sites, &SuffixList::built_in())
        };
        assert_eq!(
            of(Some("http://nas/files"), None, none),
            ChosenBy::LocalPage
        );
        assert_eq!(
            of(None, Some("http://10.0.0.5/"), none),
            ChosenBy::LocalPage
        );
        // A public page with a local referrer is a website's.
        assert_eq!(
            of(Some("https://evil.example.org/"), Some("http://nas/"), none),
            ChosenBy::Website
        );
        assert_eq!(of(None, None, none), ChosenBy::Website);
        assert_eq!(of(Some("not a link"), None, none), ChosenBy::Website);
        // A listed site and the names under it, never a look-alike.
        assert_eq!(
            of(Some("https://wiki.corp.example.com/p"), None, none),
            ChosenBy::Website
        );
        assert_eq!(
            of(Some("https://wiki.corp.example.com/p"), None, &listed),
            ChosenBy::LocalPage
        );
        assert_eq!(
            of(Some("https://corp.example.com/"), None, &listed),
            ChosenBy::LocalPage
        );
        assert_eq!(
            of(Some("https://evilcorp.example.com/"), None, &listed),
            ChosenBy::Website
        );
    }

    // A site listed while one owner held it stops counting once a newer list makes it a
    // public suffix, as stored settings are read with the list in use, since the names
    // under it are then every customer's sites (D39.4, D44).
    #[test]
    fn a_listed_site_a_newer_list_makes_public_no_longer_counts() {
        let listed = ["corp.example.com".to_string()];
        let page = || capture(Some("https://wiki.corp.example.com/p"), None);
        let built_in = SuffixList::built_in();
        assert_eq!(
            ChosenBy::of_capture(&page(), &listed, &built_in),
            ChosenBy::LocalPage
        );
        let newer = crate::public_suffix::testing::list(
            crate::public_suffix::testing::NEWER,
            &["corp.example.com"],
        );
        let newer = SuffixList::accept(
            newer.as_bytes(),
            &built_in,
            crate::public_suffix::testing::NEWER_MILLIS,
        )
        .unwrap();
        assert_eq!(
            ChosenBy::of_capture(&page(), &listed, &newer),
            ChosenBy::Website
        );
    }

    /// A frame from another site on a local page, or a referrer from one,
    /// started the capture: the frame chose the link, not the local page.
    #[test]
    fn every_page_address_a_capture_carries_must_be_local() {
        let none: &[String] = &[];
        let of = |page: Option<&str>, frame: Option<&str>, referrer: Option<&str>| {
            let mut capture = capture(page, referrer);
            capture.frame_url = frame.map(str::to_string);
            ChosenBy::of_capture(&capture, none, &SuffixList::built_in())
        };
        let portal = Some("http://portal/home");
        let vendor = Some("https://media.vendorcdn.com/player");
        assert_eq!(of(portal, None, vendor), ChosenBy::Website);
        assert_eq!(of(portal, vendor, None), ChosenBy::Website);
        assert_eq!(of(portal, vendor, portal), ChosenBy::Website);
        assert_eq!(of(portal, Some("not a link"), None), ChosenBy::Website);
        assert_eq!(of(vendor, portal, portal), ChosenBy::Website);
        assert_eq!(
            of(portal, Some("http://nas/frame"), portal),
            ChosenBy::LocalPage
        );
        assert_eq!(of(None, portal, None), ChosenBy::LocalPage);
    }

    /// An extension from before `frame_url` sends a context-menu link from an
    /// https frame to an http address with no referrer at all, so the local
    /// tab would be the only address left: the frame's missing trace must not
    /// count as local.
    #[test]
    fn a_capture_that_may_come_from_a_frame_needs_a_trace_of_it() {
        let none: &[String] = &[];
        let of = |source, frame: Option<&str>, referrer: Option<&str>| {
            let mut capture = capture(Some("http://portal/home"), referrer);
            capture.source = source;
            capture.frame_url = frame.map(str::to_string);
            ChosenBy::of_capture(&capture, none, &SuffixList::built_in())
        };
        for source in [CaptureSource::ContextMenu, CaptureSource::MediaButton] {
            assert_eq!(of(source, None, None), ChosenBy::Website, "{source:?}");
            assert_eq!(
                of(source, None, Some("http://portal/")),
                ChosenBy::LocalPage,
                "{source:?}"
            );
            assert_eq!(
                of(source, Some("http://portal/home"), None),
                ChosenBy::LocalPage,
                "{source:?}"
            );
        }
        // A browser download's page address is its referrer, the frame's own.
        assert_eq!(
            of(CaptureSource::BrowserDownload, None, None),
            ChosenBy::LocalPage
        );
    }

    #[tokio::test]
    async fn only_a_websites_link_into_the_local_network_is_refused() {
        for link in [
            "http://192.168.1.1/a",
            "http://localhost/a",
            "http://[::1]/a",
        ] {
            assert!(refuses(ChosenBy::Website, &url(link)).await, "{link}");
            assert!(!refuses(ChosenBy::LocalPage, &url(link)).await, "{link}");
            assert!(!refuses(ChosenBy::Person, &url(link)).await, "{link}");
        }
        assert!(!refuses(ChosenBy::Website, &url("http://93.184.215.14/a")).await);
        // A name that does not resolve is the engine's to decide.
        assert!(!refuses(ChosenBy::Website, &url("http://no-such-host.invalid/a")).await);
        assert!(refuses_address(
            ChosenBy::Website,
            &url("http://10.0.0.1/a")
        ));
        assert!(!refuses_address(
            ChosenBy::Website,
            &url("http://localhost/a")
        ));
        assert!(!refuses_address(
            ChosenBy::LocalPage,
            &url("http://10.0.0.1/a")
        ));
    }
}
