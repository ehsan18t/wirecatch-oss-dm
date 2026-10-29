//! Which proxy a request goes through, read once per download and asked by the
//! HTTP client for every request and by the engine's own decisions, so they
//! always agree.
//!
//! Some of the engine's decisions depend on whether a request goes through a
//! proxy: HTTP/3 is not used then (QUIC would go around the proxy), FTP is
//! refused (it cannot go through one yet), and the resolver that keeps
//! downloads off the local network trusts the proxies' own names. The HTTP
//! client never reads a proxy setting itself: it is handed [`Proxies::client`],
//! which asks the same settings.
//!
//! The environment comes first. A scheme's own variable (`HTTPS_PROXY` or
//! `https_proxy` for `https`) counts, then `ALL_PROXY` (or `all_proxy`), and a
//! host that `NO_PROXY` (or `no_proxy`) names is reached directly. `NO_PROXY`
//! is matched by `hyper-util`'s matcher, the one reqwest uses: a list of names
//! and addresses separated by commas, where a name also covers its subdomains
//! (`example.com` and `.example.com` both cover `files.example.com`), an
//! address or a network (`10.0.0.0/8`) covers the addresses in it, and `*`
//! covers every name. In a CGI program (`REQUEST_METHOD` is set) no proxy is
//! read at all, as in the HTTP client, since a request's `Proxy:` header
//! arrives there as `HTTP_PROXY`.
//!
//! When no proxy variable is set, the system's proxy setting counts while it is
//! turned on: on Windows, the manual proxy in the network settings, for every
//! protocol or per protocol, with its exceptions (unless `NO_PROXY` is set).
//! Other systems have no setting the engine reads.
//!
//! All of that is [`ProxyChoice::System`], the default. An application with a
//! proxy setting of its own gives [`ProxyChoice::Direct`] or
//! [`ProxyChoice::Manual`] instead, and then neither the environment nor the
//! system's setting is read.

use std::ffi::OsString;
use std::fmt;
use std::sync::Arc;

use hyper_util::client::proxy::matcher::Matcher;
use url::{Host, Url};

use crate::socks::Socks5;

/// Reads an environment variable; tests pass their own.
pub(crate) type Env<'a> = &'a (dyn Fn(&str) -> Option<OsString> + Sync);

/// Reads the process's environment.
pub(crate) fn process_env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// Where the proxy a request would go through is set.
///
/// New variants may be added in minor releases, so a `match` needs a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProxySetting {
    /// An environment variable, named as it was read (`ALL_PROXY`,
    /// `ftp_proxy`, ...).
    Variable(String),
    /// The system's proxy setting (on Windows, the manual proxy in the network
    /// settings).
    System,
    /// The proxy given in [`DownloadOptions::proxy`](crate::DownloadOptions::proxy).
    Given,
}

impl ProxySetting {
    /// How to stop a request from using this setting's proxy.
    pub(crate) fn remedy(&self) -> String {
        match self {
            Self::Variable(name) => format!("remove {name} from the environment"),
            Self::System => "turn the proxy off in the system's network settings, or add this server to its exceptions".to_string(),
            Self::Given => "add this server to the proxy's exceptions, or let FTP links go directly".to_string(),
        }
    }
}

impl fmt::Display for ProxySetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Variable(name) => write!(f, "the {name} environment variable"),
            Self::System => f.write_str("the system's proxy setting"),
            Self::Given => f.write_str("the download's proxy option"),
        }
    }
}

/// Which proxies a download's requests go through ([`DownloadOptions::proxy`](crate::DownloadOptions::proxy)).
///
/// New variants may be added in minor releases, so a `match` needs a `_` arm.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProxyChoice {
    /// The proxy variables of the environment, or else the system's proxy
    /// setting (see the [crate documentation](crate#proxies)).
    #[default]
    System,
    /// No proxy: every request goes directly, whatever the environment or the
    /// system sets.
    Direct,
    /// The proxies given, and neither the environment nor the system's setting.
    Manual(ManualProxies),
}

/// Proxies an application sets itself ([`ProxyChoice::Manual`]).
///
/// Each proxy is a URL, as in the proxy variables: `http://`, `https://`,
/// `socks4://`, `socks5://` (names resolved here) or `socks5h://` (names
/// resolved by the proxy), with a login as `user:password@` when the proxy
/// asks for one. A value without a scheme is an `http` proxy. An empty value
/// is no proxy.
///
/// New fields may be added in minor releases: start from
/// [`ManualProxies::default`].
#[derive(Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ManualProxies {
    /// For every protocol without a proxy of its own below.
    pub all: Option<String>,
    /// For `http` links.
    pub http: Option<String>,
    /// For `https` links.
    pub https: Option<String>,
    /// For `ftp`, `ftps` and `ftpes` links. Only a SOCKS5 proxy can carry FTP.
    pub ftp: Option<String>,
    /// The hosts reached directly, separated by commas or semicolons: names
    /// (each covers its subdomains, written `example.com`, `.example.com` or
    /// `*.example.com`), addresses, networks (`10.0.0.0/8` or `10.*`), `*`
    /// for every name, and `<local>` for every name without a dot.
    pub exceptions: String,
    /// An FTP link whose proxy cannot carry FTP (anything but SOCKS5) goes
    /// directly instead of failing with
    /// [`Error::FtpProxyUnsupported`](crate::Error::FtpProxyUnsupported).
    pub ftp_direct: bool,
}

/// Shows the proxies without the logins their values may carry.
impl fmt::Debug for ManualProxies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = |value: &Option<String>| value.as_deref().map(without_login);
        f.debug_struct("ManualProxies")
            .field("all", &shown(&self.all))
            .field("http", &shown(&self.http))
            .field("https", &shown(&self.https))
            .field("ftp", &shown(&self.ftp))
            .field("exceptions", &self.exceptions)
            .field("ftp_direct", &self.ftp_direct)
            .finish()
    }
}

/// How a request for a link goes ([`ProxyChoice::route`]).
///
/// New variants may be added in minor releases, so a `match` needs a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProxyRoute {
    /// Directly: no proxy is set for its protocol, or an FTP link may go around a proxy
    /// that cannot carry it.
    Direct,
    /// Directly, because its host is on the exceptions of the proxy set for it.
    Excepted,
    /// Through this proxy, written without its login.
    Through(String),
}

impl ProxyChoice {
    /// How a request for `url` goes, reading the environment and the system's setting now
    /// for [`ProxyChoice::System`].
    #[must_use]
    pub fn route(&self, url: &Url) -> ProxyRoute {
        Proxies::of(self).route(url)
    }

    /// Whether every proxy given can be read. The HTTP client goes directly
    /// around one it cannot read, which an application's own setting must
    /// never do; a value from the environment keeps D62's behaviour.
    pub(crate) fn readable(&self) -> bool {
        match self {
            Self::Manual(manual) => {
                let proxies = Proxies::manual(manual);
                [&proxies.http, &proxies.https, &proxies.ftp, &proxies.all]
                    .into_iter()
                    .flatten()
                    .all(|proxy| proxy.url().is_some())
            }
            _ => true,
        }
    }
}

impl ManualProxies {
    /// What [`ProxyChoice::System`] reads now, as manual proxies, or `None` when
    /// it finds no proxy: for an application that offers to copy the system's
    /// setting into its own.
    #[must_use]
    pub fn from_system() -> Option<Self> {
        Proxies::of(&ProxyChoice::System).as_manual()
    }

    /// The hosts the computer reaches directly, in the form [`exceptions`](Self::exceptions)
    /// takes: `NO_PROXY` (or `no_proxy`), or else on Windows the exceptions of the system's
    /// proxy setting, whether that proxy is turned on or not. Empty when neither sets any.
    #[must_use]
    pub fn system_exceptions() -> String {
        system_exceptions(&process_env, system_exception_list)
    }
}

/// The exceptions `env` sets, or else those `system` reads.
pub(crate) fn system_exceptions(env: Env<'_>, system: impl FnOnce() -> Option<String>) -> String {
    if env("REQUEST_METHOD").is_some() {
        return String::new();
    }
    match first_set(&["NO_PROXY", "no_proxy"], env) {
        Some((_, value)) if !value.trim().is_empty() => value,
        _ => system().unwrap_or_default(),
    }
}

/// The exceptions of the system's proxy setting, read while the proxy is turned off too.
#[cfg(windows)]
fn system_exception_list() -> Option<String> {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?
        .get_string("ProxyOverride")
        .ok()
}

/// The exceptions of the system's proxy setting: none outside Windows.
#[cfg(not(windows))]
fn system_exception_list() -> Option<String> {
    None
}

/// A proxy value with its login, if any, shown as `***`.
fn without_login(value: &str) -> String {
    match value.rsplit_once('@') {
        Some((before, host)) => match before.split_once("://") {
            Some((scheme, _)) => format!("{scheme}://***@{host}"),
            None => format!("***@{host}"),
        },
        None => value.to_string(),
    }
}

/// What a request is sent with, as far as proxies go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Http,
    Https,
    Ftp,
}

impl Scheme {
    fn of(url: &Url) -> Option<Self> {
        match url.scheme() {
            "http" => Some(Self::Http),
            "https" => Some(Self::Https),
            _ if crate::url::is_ftp(url) => Some(Self::Ftp),
            _ => None,
        }
    }
}

/// One proxy: the value as it was set, and where.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Proxy {
    value: String,
    pub(crate) setting: ProxySetting,
}

/// Shows the proxy without the user name and password its value may carry.
impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proxy")
            .field("value", &without_login(&self.value))
            .field("setting", &self.setting)
            .finish()
    }
}

impl Proxy {
    /// The proxy as the HTTP client takes it, or `None` when the client
    /// cannot read it. Like the client, a value without a scheme is an http
    /// proxy.
    fn url(&self) -> Option<Url> {
        let readable = Matcher::builder()
            .all(self.value.clone())
            .build()
            .intercept(&::http::Uri::from_static("http://destination.invalid/"))
            .is_some();
        if !readable {
            return None;
        }
        if self.value.contains("://") {
            Url::parse(&self.value).ok()
        } else {
            Url::parse(&format!("http://{}", self.value)).ok()
        }
    }
}

/// The system's proxy setting while it is turned on, as the system stores it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SystemProxy {
    /// The proxy for every protocol (`proxy:8080`), or one per protocol
    /// (`http=proxy:80;https=proxy:443;ftp=ftp-proxy:21`), where a `socks`
    /// entry serves every protocol without its own.
    pub servers: String,
    /// The hosts reached directly, separated by semicolons: names (`*.corp`
    /// covers its subdomains), addresses, IPv4 wildcards (`10.*`) and
    /// `<local>`, every name without a dot.
    pub exceptions: String,
}

/// Reads the system's proxy setting; `None` while it is turned off.
#[cfg(windows)]
pub(crate) fn system_proxy() -> Option<SystemProxy> {
    let settings = windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    if settings.get_u32("ProxyEnable").unwrap_or(0) == 0 {
        return None;
    }
    Some(SystemProxy {
        servers: settings.get_string("ProxyServer").unwrap_or_default(),
        exceptions: settings.get_string("ProxyOverride").unwrap_or_default(),
    })
}

/// Reads the system's proxy setting: none outside Windows.
#[cfg(not(windows))]
pub(crate) fn system_proxy() -> Option<SystemProxy> {
    None
}

/// The proxy settings a download's requests follow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Proxies {
    http: Option<Proxy>,
    https: Option<Proxy>,
    ftp: Option<Proxy>,
    all: Option<Proxy>,
    /// The hosts reached directly, in `NO_PROXY` form.
    no: String,
    /// Names without a dot are reached directly (the system's `<local>`).
    plain_names_direct: bool,
    /// An FTP link whose proxy cannot carry FTP goes directly.
    ftp_direct: bool,
}

impl Proxies {
    /// The settings `choice` asks for, read now.
    pub(crate) fn of(choice: &ProxyChoice) -> Self {
        match choice {
            ProxyChoice::System => Self::read(&process_env, system_proxy),
            ProxyChoice::Direct => Self::default(),
            ProxyChoice::Manual(manual) => Self::manual(manual),
        }
    }

    /// The proxies an application set.
    fn manual(manual: &ManualProxies) -> Self {
        let proxy = |value: &Option<String>| {
            let value = value.as_deref()?.trim();
            (!value.is_empty()).then(|| Proxy {
                value: value.to_string(),
                setting: ProxySetting::Given,
            })
        };
        let (no, plain_names_direct) = exceptions(&manual.exceptions);
        Self {
            http: proxy(&manual.http),
            https: proxy(&manual.https),
            ftp: proxy(&manual.ftp),
            all: proxy(&manual.all),
            no,
            plain_names_direct,
            ftp_direct: manual.ftp_direct,
        }
    }

    /// The settings `env` gives, or else those `system` reads.
    pub(crate) fn read(env: Env<'_>, system: impl FnOnce() -> Option<SystemProxy>) -> Self {
        if env("REQUEST_METHOD").is_some() {
            return Self::default();
        }
        let variable = |pair: [&'static str; 2]| {
            let (name, value) = first_set(&pair, env)?;
            (!value.trim().is_empty()).then(|| Proxy {
                value,
                setting: ProxySetting::Variable(name.to_string()),
            })
        };
        let mut proxies = Self {
            http: variable(["HTTP_PROXY", "http_proxy"]),
            https: variable(["HTTPS_PROXY", "https_proxy"]),
            ftp: variable(["FTP_PROXY", "ftp_proxy"]),
            all: variable(["ALL_PROXY", "all_proxy"]),
            no: first_set(&["NO_PROXY", "no_proxy"], env)
                .map(|(_, value)| value)
                .unwrap_or_default(),
            plain_names_direct: false,
            ftp_direct: false,
        };
        let none_set = [&proxies.http, &proxies.https, &proxies.ftp, &proxies.all]
            .iter()
            .all(|proxy| proxy.is_none());
        if none_set && let Some(system) = system() {
            proxies.adopt(&system);
        }
        proxies
    }

    /// Takes the system's setting.
    fn adopt(&mut self, system: &SystemProxy) {
        let proxy = |value: &str| {
            let value = value.trim();
            (!value.is_empty()).then(|| Proxy {
                value: value.to_string(),
                setting: ProxySetting::System,
            })
        };
        if system.servers.contains('=') {
            for entry in system.servers.split(';') {
                let Some((protocol, server)) = entry.split_once('=') else {
                    continue;
                };
                match protocol.trim().to_ascii_lowercase().as_str() {
                    "http" => self.http = proxy(server),
                    "https" => self.https = proxy(server),
                    "ftp" => self.ftp = proxy(server),
                    // As the system uses it: for every protocol without its
                    // own entry, and SOCKS4, the only version it speaks.
                    "socks" => self.all = proxy(&format!("socks4://{}", server.trim())),
                    _ => {}
                }
            }
        } else {
            self.all = proxy(&system.servers);
        }
        if self.no.trim().is_empty() {
            (self.no, self.plain_names_direct) = exceptions(&system.exceptions);
        }
    }

    /// These settings as manual proxies, or `None` when they hold no proxy.
    fn as_manual(&self) -> Option<ManualProxies> {
        let value = |proxy: &Option<Proxy>| proxy.as_ref().map(|proxy| proxy.value.clone());
        let mut exceptions = self.no.clone();
        if self.plain_names_direct {
            if !exceptions.trim().is_empty() {
                exceptions.push(',');
            }
            exceptions.push_str("<local>");
        }
        let manual = ManualProxies {
            all: value(&self.all),
            http: value(&self.http),
            https: value(&self.https),
            ftp: value(&self.ftp),
            exceptions,
            ftp_direct: self.ftp_direct,
        };
        [&manual.all, &manual.http, &manual.https, &manual.ftp]
            .iter()
            .any(|proxy| proxy.is_some())
            .then_some(manual)
    }

    /// The proxy a request with `scheme` to `host` goes through, or `None`
    /// when it goes directly: the first the HTTP client can read of the
    /// scheme's own and the one for every protocol, as the client did when it
    /// read the settings itself. When it can read neither, the one that is set
    /// still counts, the careful answer for a decision that would otherwise go
    /// around a proxy the user set; the client itself then goes directly.
    pub(crate) fn proxy_for(&self, scheme: Scheme, host: &str) -> Option<&Proxy> {
        let set = [self.own(scheme), self.all.as_ref()].into_iter().flatten();
        let proxy = set
            .clone()
            .find(|proxy| proxy.url().is_some())
            .or_else(|| set.clone().next())?;
        if self.plain_names_direct && !host.contains('.') && !host.contains(':') {
            return None;
        }
        (!excluded(&proxy.value, &self.no, host)).then_some(proxy)
    }

    /// The proxy set for `scheme` alone.
    fn own(&self, scheme: Scheme) -> Option<&Proxy> {
        match scheme {
            Scheme::Http => self.http.as_ref(),
            Scheme::Https => self.https.as_ref(),
            Scheme::Ftp => self.ftp.as_ref(),
        }
    }

    fn route(&self, url: &Url) -> ProxyRoute {
        let (Some(scheme), Some(host)) = (Scheme::of(url), url.host_str()) else {
            return ProxyRoute::Direct;
        };
        match self.proxy_for(scheme, host) {
            Some(_) if scheme == Scheme::Ftp && matches!(self.ftp_route(host), Ok(None)) => {
                ProxyRoute::Direct
            }
            Some(proxy) => ProxyRoute::Through(without_login(&proxy.value)),
            None if self.own(scheme).is_some() || self.all.is_some() => ProxyRoute::Excepted,
            None => ProxyRoute::Direct,
        }
    }

    /// The proxy the HTTP client sends a request for `url` through.
    fn client_target(&self, url: &Url) -> Option<Url> {
        let scheme = Scheme::of(url).filter(|scheme| *scheme != Scheme::Ftp)?;
        self.proxy_for(scheme, url.host_str()?)?.url()
    }

    /// How an FTP connection to `host` goes: directly (`Ok(None)`), through
    /// a SOCKS5 proxy, or not at all, when its proxy cannot carry FTP and
    /// going around it was not allowed (`Err`, naming where it is set).
    pub(crate) fn ftp_route(&self, host: &str) -> Result<Option<Socks5>, ProxySetting> {
        let Some(proxy) = self.proxy_for(Scheme::Ftp, host) else {
            return Ok(None);
        };
        match proxy.url().as_ref().and_then(Socks5::from_url) {
            Some(socks) => Ok(Some(socks)),
            None if self.ftp_direct => Ok(None),
            None => Err(proxy.setting.clone()),
        }
    }

    /// These settings as the HTTP client's only proxy, so it reads none of
    /// its own.
    pub(crate) fn client(self: &Arc<Self>) -> reqwest::Proxy {
        let proxies = self.clone();
        reqwest::Proxy::custom(move |url| proxies.client_target(url))
    }

    /// The host names of every proxy the HTTP client may use (for `http` and
    /// `https`). The client connects to these itself, so their names reach the
    /// name resolver; a proxy given as an IP address needs no entry.
    pub(crate) fn host_names(&self) -> Vec<String> {
        let mut hosts: Vec<String> = Vec::new();
        for proxy in [&self.http, &self.https, &self.all].into_iter().flatten() {
            let value = proxy.value.trim();
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
        hosts
    }
}

/// The first of `names` that is set to text, with its value. A value that is
/// not Unicode counts as not set, as in the HTTP client.
fn first_set(names: &[&'static str], env: Env<'_>) -> Option<(&'static str, String)> {
    names
        .iter()
        .find_map(|&name| env(name)?.into_string().ok().map(|value| (name, value)))
}

/// Exceptions as the system writes them (separated by semicolons) or as an
/// application gives them (by commas, as in `NO_PROXY`), in `NO_PROXY` form,
/// and whether they hold `<local>`. `*.corp.example` becomes `corp.example`, which covers its
/// subdomains (and the name itself), an IPv4 wildcard becomes the network it
/// covers, and `*` (or `*.*`) covers every name, as in `NO_PROXY`; any other
/// pattern stays as it is and matches nothing, so its hosts keep the proxy.
fn exceptions(list: &str) -> (String, bool) {
    let mut local = false;
    let mut entries = Vec::new();
    for entry in list
        .split([';', ','])
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        if entry.eq_ignore_ascii_case("<local>") {
            local = true;
        } else if let Some(network) = ipv4_wildcard(entry) {
            entries.push(network);
        } else {
            entries.push(entry.strip_prefix("*.").unwrap_or(entry).to_string());
        }
    }
    (entries.join(","), local)
}

/// `10.*` or `192.168.*.*` as the network it covers (`10.0.0.0/8`,
/// `192.168.0.0/16`), or `None` for anything else.
fn ipv4_wildcard(entry: &str) -> Option<String> {
    let parts: Vec<&str> = entry.split('.').collect();
    let first_wild = parts.iter().position(|part| *part == "*")?;
    if first_wild == 0 || parts.len() > 4 || parts[first_wild..].iter().any(|part| *part != "*") {
        return None;
    }
    let mut octets = [0u8; 4];
    for (octet, part) in octets.iter_mut().zip(&parts[..first_wild]) {
        *octet = part.parse().ok()?;
    }
    let [a, b, c, d] = octets;
    Some(format!("{a}.{b}.{c}.{d}/{}", first_wild * 8))
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
mod tests;
