//! Proxies (D73) as the engine, yt-dlp and the updater take them: the checks of a profile and
//! of a list of exceptions, a profile's saved password, and copying the computer's proxy.

use std::net::IpAddr;

use wirecatch_engine::{ManualProxies, ProxyChoice, Url};
use wirecatch_protocol::Secret;
use wirecatch_protocol::app::{Setting, Update};
use wirecatch_protocol::proxy::{
    MAX_PROXY_EXCEPTIONS, MAX_PROXY_HOST_CHARS, MAX_PROXY_LOGIN_BYTES, MAX_PROXY_NAME_CHARS,
    MAX_PROXY_SERVERS, ProxyKind, ProxyServerSettings, ProxySettings, ProxyUse, QueueExceptions,
};

pub use wirecatch_records::proxy::{Logins, SavedProxyServer};

use crate::error::{Result, SettingsError};

/// Checks the app's proxy and cleans up its hosts. Whether its profile exists is
/// [`check_use`]'s to say.
///
/// # Errors
///
/// [`SettingsError::Invalid`] naming the first host that cannot be an exception.
pub fn validate(mut proxy: ProxySettings) -> Result<ProxySettings> {
    proxy.exceptions.hosts = clean_hosts(&proxy.exceptions.hosts)?;
    Ok(proxy)
}

/// Checks a queue's own exceptions and cleans up their hosts.
///
/// # Errors
///
/// [`SettingsError::Invalid`] naming the first host that cannot be an exception.
pub fn validate_queue_exceptions(
    exceptions: Setting<QueueExceptions>,
) -> Result<Setting<QueueExceptions>> {
    Ok(match exceptions {
        Setting::Global => Setting::Global,
        Setting::Own(mut own) => {
            own.hosts = clean_hosts(&own.hosts)?;
            Setting::Own(own)
        }
    })
}

/// Hosts trimmed, in lower case and without duplicates.
fn clean_hosts(hosts: &[String]) -> Result<Vec<String>> {
    let mut cleaned: Vec<String> = Vec::new();
    for entry in hosts {
        let entry = entry.trim().to_ascii_lowercase();
        if entry.is_empty() || cleaned.contains(&entry) {
            continue;
        }
        if entry.len() > MAX_PROXY_HOST_CHARS
            || entry.contains(|c: char| c.is_whitespace() || c.is_control() || c == ',' || c == ';')
        {
            return Err(invalid(&format!(
                "\"{entry}\" cannot be a proxy exception. Use a host name like files.example.com, an address, or a network like 10.0.0.0/8."
            )));
        }
        cleaned.push(entry);
    }
    if cleaned.len() > MAX_PROXY_EXCEPTIONS {
        return Err(invalid(&format!(
            "A list of proxy exceptions can have at most {MAX_PROXY_EXCEPTIONS} hosts. Remove some first."
        )));
    }
    Ok(cleaned)
}

/// Checks that `count` proxy servers are allowed.
///
/// # Errors
///
/// [`SettingsError::LimitReached`] past [`MAX_PROXY_SERVERS`].
pub fn check_count(count: usize) -> Result<()> {
    if count > MAX_PROXY_SERVERS {
        return Err(SettingsError::LimitReached(format!(
            "There can be at most {MAX_PROXY_SERVERS} proxy servers. Delete one first."
        )));
    }
    Ok(())
}

/// Checks that `proxy` names a saved profile, when it names one.
///
/// # Errors
///
/// [`SettingsError::Invalid`] for a profile that is not saved.
pub fn check_use(proxy: ProxyUse, profiles: &[SavedProxyServer]) -> Result<()> {
    match proxy {
        ProxyUse::Server(id) if !profiles.iter().any(|p| p.id == id) => Err(invalid(
            "That proxy server no longer exists. Choose another proxy.",
        )),
        _ => Ok(()),
    }
}

/// Checks a profile and cleans it up: its name and host trimmed. `others` are the saved
/// profiles but this one, whose names it may not take.
///
/// # Errors
///
/// [`SettingsError::Invalid`] naming the first value that cannot work.
pub fn validate_settings_file(
    mut profile: ProxyServerSettings,
    others: &[&ProxyServerSettings],
) -> Result<ProxyServerSettings> {
    profile.name = profile.name.trim().to_string();
    profile.host = profile.host.trim().to_string();
    if profile.name.chars().count() > MAX_PROXY_NAME_CHARS {
        return Err(invalid(&format!(
            "Proxy names can be at most {MAX_PROXY_NAME_CHARS} characters."
        )));
    }
    check_server(&profile)?;
    let shown = profile.shown_name();
    if others
        .iter()
        .any(|other| other.shown_name().eq_ignore_ascii_case(&shown))
    {
        return Err(invalid(&format!(
            "A proxy named \"{shown}\" is already saved. Give this one another name."
        )));
    }
    Ok(profile)
}

fn check_server(server: &ProxyServerSettings) -> Result<()> {
    let host = &server.host;
    if host.is_empty() {
        return Err(invalid("Enter the proxy server's name or address."));
    }
    let readable = host.len() <= MAX_PROXY_HOST_CHARS
        && (host.parse::<IpAddr>().is_ok()
            || host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_'));
    if !readable {
        return Err(invalid(&format!(
            "\"{host}\" is not a proxy server name. Enter a host name or address only, without http:// or a port."
        )));
    }
    if server.port == 0 {
        return Err(invalid(&format!(
            "Enter the port of the proxy server {host}."
        )));
    }
    // Digits and dots read as an address, and one out of range fits no URL.
    if Url::parse(&format!("http://{}", server.address())).is_err() {
        return Err(invalid(&format!(
            "\"{host}\" is not a proxy server name or address. Check it for a mistyped number."
        )));
    }
    if server.user.len() > MAX_PROXY_LOGIN_BYTES || server.user.contains(char::is_control) {
        return Err(invalid(&format!(
            "The user name for the proxy server {host} is too long or holds a line break."
        )));
    }
    Ok(())
}

fn invalid(message: &str) -> SettingsError {
    SettingsError::Invalid(message.to_string())
}

/// A profile's password after `change`, for the profile `was` (`None` for a new one) becoming
/// `now`. A password kept as it is stays only while the profile is the same proxy (type,
/// server, port and user name): it was given for that proxy, never for another.
///
/// # Errors
///
/// [`SettingsError::Invalid`] for a password the proxy cannot be sent.
pub fn merge_password(
    saved: Option<&Secret>,
    change: &Update<Option<Secret>>,
    was: Option<&ProxyServerSettings>,
    now: &ProxyServerSettings,
) -> Result<Option<Secret>> {
    match change {
        Update::Keep => Ok(saved
            .filter(|_| was.is_some_and(|was| same_proxy(was, now)))
            .cloned()),
        Update::Set(Some(password)) if !password.expose().is_empty() => {
            if now.user.is_empty() {
                return Err(invalid(
                    "Enter the user name that goes with the proxy password.",
                ));
            }
            let text = password.expose();
            if text.len() > MAX_PROXY_LOGIN_BYTES || text.contains(char::is_control) {
                return Err(invalid(
                    "A proxy password is too long or holds a line break.",
                ));
            }
            Ok(Some(password.clone()))
        }
        Update::Set(_) => Ok(None),
    }
}

/// Whether `a` and `b` name the same proxy and login, so a password given for one is the other's.
#[must_use]
pub fn same_proxy(a: &ProxyServerSettings, b: &ProxyServerSettings) -> bool {
    a.kind == b.kind
        && a.host.trim().eq_ignore_ascii_case(b.host.trim())
        && a.port == b.port
        && a.user == b.user
}

/// The hosts a queue's downloads reach directly: the computer's bypass list or not, and the
/// hosts listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exceptions {
    pub system: bool,
    pub hosts: Vec<String>,
}

/// The exceptions a queue's `own` (or the app's, when it follows them) come to (D73.7).
#[must_use]
pub fn exceptions(app: &ProxySettings, own: &Setting<QueueExceptions>) -> Exceptions {
    let app_hosts = || {
        if app.exceptions.use_hosts {
            app.exceptions.hosts.clone()
        } else {
            Vec::new()
        }
    };
    match own {
        Setting::Global => Exceptions {
            system: app.exceptions.system,
            hosts: app_hosts(),
        },
        Setting::Own(own) => {
            let mut hosts = if own.app_hosts {
                app_hosts()
            } else {
                Vec::new()
            };
            if own.use_hosts {
                hosts.extend(own.hosts.iter().cloned());
            }
            Exceptions {
                system: own.system,
                hosts,
            }
        }
    }
}

/// Where the computer's proxy and bypass list (D73.6) come from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ComputerProxy {
    /// The environment's proxy variables, else the system's setting, read when asked.
    #[default]
    Read,
    /// This setting, and nothing read from the computer: `None` for no proxy and no bypass list,
    /// or proxies whose `exceptions` are the bypass list (all proxies `None` for a list alone).
    /// Handed to the engine as given proxies, so its messages name the download's proxy option.
    Given(Option<ManualProxies>),
}

impl ComputerProxy {
    /// The computer's proxy, or `None` when it sets none.
    #[must_use]
    pub fn proxies(&self) -> Option<ManualProxies> {
        match self {
            Self::Read => ManualProxies::from_system(),
            Self::Given(given) => given.clone(),
        }
    }

    /// The computer's bypass list.
    #[must_use]
    pub fn exceptions(&self) -> String {
        match self {
            Self::Read => ManualProxies::system_exceptions(),
            Self::Given(given) => given
                .as_ref()
                .map(|given| given.exceptions.clone())
                .unwrap_or_default(),
        }
    }

    /// The computer's proxy with nothing added, as the engine takes it.
    #[must_use]
    pub fn choice(&self) -> ProxyChoice {
        match self {
            Self::Read => ProxyChoice::System,
            Self::Given(given) => given
                .clone()
                .map_or(ProxyChoice::Direct, ProxyChoice::Manual),
        }
    }
}

/// The proxies the engine follows for `proxy` with `exceptions`. `profiles` and `logins` are the
/// saved profiles and their passwords.
#[must_use]
pub fn engine_choice(
    proxy: ProxyUse,
    exceptions: &Exceptions,
    ftp_bypass_proxy: bool,
    profiles: &[SavedProxyServer],
    logins: &Logins,
    computer: &ComputerProxy,
) -> ProxyChoice {
    let manual = |mut manual: ManualProxies| {
        manual.exceptions = exception_list(exceptions, computer);
        manual.ftp_direct = ftp_bypass_proxy;
        ProxyChoice::Manual(manual)
    };
    match proxy {
        ProxyUse::Direct => ProxyChoice::Direct,
        // As D62 reads it, unless something is added to what it reads.
        ProxyUse::System
            if exceptions.system && exceptions.hosts.is_empty() && !ftp_bypass_proxy =>
        {
            computer.choice()
        }
        // None only when the computer sets no proxy, so System would go directly too.
        ProxyUse::System => computer.proxies().map_or(ProxyChoice::Direct, manual),
        ProxyUse::Server(id) => {
            let Some(profile) = profiles.iter().find(|p| p.id == id) else {
                tracing::warn!(
                    profile = id.0,
                    "a proxy server that is not saved was chosen; following the computer's proxy instead"
                );
                return computer.choice();
            };
            let mut proxies = ManualProxies::default();
            proxies.all = profile_url(&profile.settings, logins.get(&id));
            manual(proxies)
        }
    }
}

/// `exceptions` as one list in the form [`ManualProxies::exceptions`] takes.
fn exception_list(exceptions: &Exceptions, computer: &ComputerProxy) -> String {
    let mut list = Vec::with_capacity(exceptions.hosts.len() + 1);
    if exceptions.system {
        let system = computer.exceptions();
        if !system.trim().is_empty() {
            list.push(system);
        }
    }
    list.extend(exceptions.hosts.iter().cloned());
    list.join(",")
}

/// The URL of a profile as the engine takes it, with its login.
#[must_use]
pub fn profile_url(profile: &ProxyServerSettings, password: Option<&Secret>) -> Option<String> {
    let scheme = match profile.kind {
        ProxyKind::Http => "http",
        ProxyKind::Https => "https",
        // The proxy resolves names, so they never reach this computer's name server.
        ProxyKind::Socks5 => "socks5h",
    };
    let mut url = Url::parse(&format!("{scheme}://{}", profile.address())).ok()?;
    if !profile.user.is_empty() || password.is_some() {
        // Neither fails on a URL with a host.
        url.set_username(&profile.user).ok()?;
        url.set_password(password.map(Secret::expose)).ok()?;
    }
    Some(url.into())
}

/// The proxy yt-dlp is given for `choice` (a URL, or `""` for none), `None` to leave it to read
/// the computer's proxy itself. yt-dlp takes one proxy for everything and has no exceptions, so a manual
/// setting gives it the proxy for `https`, which video sites use.
#[must_use]
pub fn ytdlp_proxy(choice: &ProxyChoice) -> Option<String> {
    match choice {
        ProxyChoice::Manual(manual) => Some(
            manual
                .https
                .clone()
                .or_else(|| manual.all.clone())
                .or_else(|| manual.http.clone())
                .unwrap_or_default(),
        ),
        ProxyChoice::Direct => Some(String::new()),
        _ => None,
    }
}

/// The proxy the app updater uses for `choice`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdaterProxy {
    /// The computer's proxy, read by the updater.
    System,
    /// No proxy.
    Direct,
    /// This proxy.
    Url(Url),
}

/// The proxy the app updater uses: the one for `https`.
#[must_use]
pub fn updater_proxy(choice: &ProxyChoice) -> UpdaterProxy {
    match choice {
        ProxyChoice::Manual(manual) => manual
            .https
            .as_ref()
            .or(manual.all.as_ref())
            .and_then(|proxy| Url::parse(proxy).ok())
            .map_or(UpdaterProxy::Direct, UpdaterProxy::Url),
        ProxyChoice::Direct => UpdaterProxy::Direct,
        _ => UpdaterProxy::System,
    }
}

/// `found` as profiles to save beside `saved`: none for a server already saved, and a name of
/// its own for one whose address another profile's name already shows. Checked whole, so
/// saving them cannot fail halfway.
///
/// # Errors
///
/// [`SettingsError::LimitReached`] when they would be too many.
pub fn copies(
    found: Vec<ProxyServerSettings>,
    saved: &[SavedProxyServer],
) -> Result<Vec<ProxyServerSettings>> {
    let mut taken: Vec<ProxyServerSettings> = saved.iter().map(|p| p.settings.clone()).collect();
    let mut copies = Vec::new();
    for mut profile in found {
        if taken.iter().any(|p| same_proxy(p, &profile)) {
            continue;
        }
        let address = profile.address();
        let mut n = 1;
        while taken
            .iter()
            .any(|p| p.shown_name().eq_ignore_ascii_case(&profile.shown_name()))
        {
            n += 1;
            profile.name = format!("{address} ({n})");
        }
        taken.push(profile.clone());
        copies.push(profile);
    }
    check_count(taken.len())?;
    Ok(copies)
}

/// A profile for each server the computer's proxy names (D73.9): the environment's proxy
/// variables, or else the system's setting. Passwords are left out, so they never reach a
/// window. Empty when the computer sets no proxy.
#[must_use]
pub fn from_system(computer: &ComputerProxy) -> Vec<ProxyServerSettings> {
    let Some(system) = computer.proxies() else {
        return Vec::new();
    };
    let mut profiles: Vec<ProxyServerSettings> = Vec::new();
    for value in [&system.all, &system.http, &system.https, &system.ftp]
        .into_iter()
        .flatten()
    {
        if let Some(profile) = parse_server(value)
            && !profiles.iter().any(|p| same_proxy(p, &profile))
        {
            profiles.push(profile);
        }
    }
    profiles
}

/// A proxy value (`http://user@proxy:8080`, `proxy:8080`, `socks5://...`) as a profile without
/// a name; its password is left out.
fn parse_server(value: &str) -> Option<ProxyServerSettings> {
    let value = value.trim();
    let url = if value.contains("://") {
        Url::parse(value)
    } else {
        Url::parse(&format!("http://{value}"))
    }
    .ok()?;
    let kind = match url.scheme() {
        "https" => ProxyKind::Https,
        "socks4" | "socks4a" | "socks5" | "socks5h" => ProxyKind::Socks5,
        _ => ProxyKind::Http,
    };
    let host = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url.port_or_known_default().unwrap_or(match kind {
        ProxyKind::Socks5 => 1080,
        _ => 8080,
    });
    let user = percent_encoding::percent_decode_str(url.username())
        .decode_utf8_lossy()
        .into_owned();
    Some(ProxyServerSettings {
        name: String::new(),
        kind,
        host,
        port,
        user,
    })
}

#[cfg(test)]
mod tests;
