//! The app's proxy setting (D71) as the engine, yt-dlp and the updater take it: its checks,
//! its saved passwords, copying the system's setting into it, and the test before it is saved.

use std::net::IpAddr;
use std::time::Duration;

use wirecatch_engine::{
    CancellationToken, Error as EngineError, Fetcher, ManualProxies, PrivateNetwork, ProxyChoice,
    Url,
};
use wirecatch_protocol::Secret;
use wirecatch_protocol::app::Update;
use wirecatch_protocol::proxy::{
    MAX_PROXY_EXCEPTIONS, MAX_PROXY_HOST_CHARS, MAX_PROXY_LOGIN_BYTES, ProxyKind, ProxyMode,
    ProxyPasswords, ProxyRole, ProxyServer, ProxySettings, ProxyTestResult,
};

pub(crate) use wirecatch_records::proxy::Logins;

use crate::error::{CoreError, Result};

/// Where the Test connection button sends its request: the project's own host, which the app
/// already reaches for the media add-on.
pub const PROXY_TEST_URL: &str = "https://github.com/";

/// How long a test may take in all.
const TEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Checks `proxy` and cleans it up: hosts trimmed, exceptions trimmed, in lower case and
/// without duplicates. A proxy whose host is empty is not set and is kept as it is.
///
/// # Errors
///
/// [`CoreError::Invalid`] naming the first value that cannot work.
pub(crate) fn validate(mut proxy: ProxySettings) -> Result<ProxySettings> {
    for role in ProxyRole::ALL {
        let server = proxy.server_mut(role);
        server.host = server.host.trim().to_string();
        if server.is_set() {
            check_server(server)?;
        }
    }
    if proxy.mode == ProxyMode::Manual
        && !proxy
            .roles_in_use()
            .iter()
            .any(|role| proxy.server(*role).is_set())
    {
        return Err(invalid(
            "Enter a proxy server, or choose another proxy option.",
        ));
    }
    let mut exceptions: Vec<String> = Vec::new();
    for entry in &proxy.exceptions {
        let entry = entry.trim().to_ascii_lowercase();
        if entry.is_empty() || exceptions.contains(&entry) {
            continue;
        }
        if entry.len() > MAX_PROXY_HOST_CHARS
            || entry.contains(|c: char| c.is_whitespace() || c.is_control() || c == ',' || c == ';')
        {
            return Err(invalid(&format!(
                "\"{entry}\" cannot be a proxy exception. Use a host name like files.example.com, an address, or a network like 10.0.0.0/8."
            )));
        }
        exceptions.push(entry);
    }
    if exceptions.len() > MAX_PROXY_EXCEPTIONS {
        return Err(invalid(&format!(
            "The proxy can have at most {MAX_PROXY_EXCEPTIONS} exceptions. Remove some first."
        )));
    }
    proxy.exceptions = exceptions;
    Ok(proxy)
}

fn check_server(server: &ProxyServer) -> Result<()> {
    let host = &server.host;
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
    let in_url = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.clone()
    };
    if Url::parse(&format!("http://{in_url}:{}", server.port)).is_err() {
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

fn invalid(message: &str) -> CoreError {
    CoreError::Invalid(message.to_string())
}

/// The saved passwords after `passwords` changed them, for the proxies `was` named becoming
/// those `now` names. A password kept as it is stays only while its proxy is the same one (type,
/// server, port and user name): it was given for that proxy, never for another.
///
/// # Errors
///
/// [`CoreError::Invalid`] for a password the proxies cannot be sent.
pub(crate) fn merge_passwords(
    saved: &Logins,
    passwords: &ProxyPasswords,
    was: &ProxySettings,
    now: &ProxySettings,
) -> Result<Logins> {
    let mut merged = saved.clone();
    for role in ProxyRole::ALL {
        match passwords.get(role) {
            Update::Keep => {
                if !same_proxy(was.server(role), now.server(role)) {
                    merged.remove(&role);
                }
            }
            Update::Set(Some(password)) if !password.expose().is_empty() => {
                if now.server(role).user.is_empty() {
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
                merged.insert(role, password.clone());
            }
            Update::Set(_) => {
                merged.remove(&role);
            }
        }
    }
    Ok(merged)
}

/// Whether `a` and `b` name the same proxy and login, so a password given for one is the other's.
fn same_proxy(a: &ProxyServer, b: &ProxyServer) -> bool {
    a.kind == b.kind
        && a.host.trim().eq_ignore_ascii_case(b.host.trim())
        && a.port == b.port
        && a.user == b.user
}

/// Sets each proxy's `has_password` from the saved passwords.
pub(crate) fn mark_passwords(proxy: &mut ProxySettings, logins: &Logins) {
    for role in ProxyRole::ALL {
        proxy.server_mut(role).has_password = logins.contains_key(&role);
    }
}

/// The proxies the engine follows for `proxy`, with its passwords.
#[must_use]
pub(crate) fn engine_choice(proxy: &ProxySettings, logins: &Logins) -> ProxyChoice {
    match proxy.mode {
        ProxyMode::System => ProxyChoice::System,
        ProxyMode::Direct => ProxyChoice::Direct,
        ProxyMode::Manual => {
            let url = |role: ProxyRole| {
                let server = proxy.server(role);
                let kind = if role == ProxyRole::Socks {
                    ProxyKind::Socks5
                } else {
                    server.kind
                };
                server_url(kind, server, logins.get(&role))
            };
            let mut manual = ManualProxies::default();
            if proxy.per_protocol {
                manual.http = url(ProxyRole::Http);
                manual.https = url(ProxyRole::Https);
                manual.ftp = url(ProxyRole::Ftp);
                manual.all = url(ProxyRole::Socks);
            } else {
                manual.all = url(ProxyRole::All);
            }
            manual.exceptions = proxy.exceptions.join(",");
            manual.ftp_direct = proxy.ftp_direct;
            ProxyChoice::Manual(manual)
        }
    }
}

/// The URL of a proxy as the engine takes it, with its login; `None` when it is not set.
fn server_url(kind: ProxyKind, server: &ProxyServer, password: Option<&Secret>) -> Option<String> {
    if !server.is_set() {
        return None;
    }
    let scheme = match kind {
        ProxyKind::Http => "http",
        ProxyKind::Https => "https",
        // The proxy resolves names, so they never reach this computer's name server.
        ProxyKind::Socks5 => "socks5h",
    };
    let host = if server.host.contains(':') {
        format!("[{}]", server.host)
    } else {
        server.host.clone()
    };
    let mut url = Url::parse(&format!("{scheme}://{host}:{}", server.port)).ok()?;
    if !server.user.is_empty() || password.is_some() {
        // Neither fails on a URL with a host.
        url.set_username(&server.user).ok()?;
        url.set_password(password.map(Secret::expose)).ok()?;
    }
    Some(url.into())
}

/// The proxy yt-dlp is given for `choice` (a URL, or `""` for none), `None` to leave it to read
/// the computer's proxy itself. yt-dlp takes one proxy for everything and has no exceptions, so a manual
/// setting gives it the proxy for `https`, which video sites use.
#[must_use]
pub(crate) fn ytdlp_proxy(choice: &ProxyChoice) -> Option<String> {
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
pub(crate) fn updater_proxy(choice: &ProxyChoice) -> UpdaterProxy {
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

/// The computer's proxy as a manual setting, for Copy from system: the environment's
/// proxy variables, or else the system's setting. Passwords are left out, so they never
/// reach a window. `None` when the computer sets no proxy.
#[must_use]
pub fn from_system() -> Option<ProxySettings> {
    let system = ManualProxies::from_system()?;
    let server = |value: &Option<String>| value.as_deref().and_then(parse_server);
    let mut proxy = ProxySettings {
        mode: ProxyMode::Manual,
        exceptions: system
            .exceptions
            .split(',')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect(),
        ..ProxySettings::default()
    };
    if system.http.is_none() && system.https.is_none() && system.ftp.is_none() {
        proxy.all = server(&system.all).unwrap_or_default();
    } else {
        // The one for every protocol fills in for the protocols without their own, or, when it
        // is a SOCKS proxy (the system's `socks=` entry), becomes the SOCKS proxy.
        let socks = system
            .all
            .as_deref()
            .is_some_and(|all| all.starts_with("socks"));
        let general = if socks { None } else { server(&system.all) };
        proxy.per_protocol = true;
        proxy.http = server(&system.http)
            .or_else(|| general.clone())
            .unwrap_or_default();
        proxy.https = server(&system.https)
            .or_else(|| general.clone())
            .unwrap_or_default();
        proxy.ftp = server(&system.ftp).or(general).unwrap_or_default();
        if socks {
            proxy.socks = server(&system.all).unwrap_or_default();
        }
    }
    Some(proxy)
}

/// A proxy value (`http://user@proxy:8080`, `proxy:8080`, `socks5://...`) as a server; its
/// password is left out.
fn parse_server(value: &str) -> Option<ProxyServer> {
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
    Some(ProxyServer {
        kind,
        host,
        port,
        user,
        has_password: false,
    })
}

/// Sends a request to `url` through each proxy `choice` sets, whatever its exceptions and
/// protocol, and says how it went (D71.5): the first that fails, or that they all worked. A
/// choice that sets none (System, No proxy) sends one request as it would go.
pub(crate) async fn test(
    choice: ProxyChoice,
    url: &Url,
    cancel: CancellationToken,
) -> ProxyTestResult {
    let ProxyChoice::Manual(manual) = &choice else {
        return test_one(choice, url, cancel).await;
    };
    let mut tried: Vec<&String> = Vec::new();
    for proxy in [&manual.all, &manual.http, &manual.https, &manual.ftp]
        .into_iter()
        .flatten()
    {
        if tried.contains(&proxy) {
            continue;
        }
        tried.push(proxy);
        let mut alone = ManualProxies::default();
        alone.all = Some(proxy.clone());
        let result = test_one(ProxyChoice::Manual(alone), url, cancel.clone()).await;
        if result != ProxyTestResult::Works {
            return result;
        }
    }
    ProxyTestResult::Works
}

/// Sends one request to `url` through `choice` and says how it went.
async fn test_one(choice: ProxyChoice, url: &Url, cancel: CancellationToken) -> ProxyTestResult {
    // The address is the app's own (`PROXY_TEST_URL`), never one a page gave.
    let rule = PrivateNetwork::Allow;
    let mut options = crate::headers::download_options(rule, choice);
    options.connect_timeout = Duration::from_secs(10);
    options.stall_timeout = Duration::from_secs(15);
    // The person waits on the answer: the first failure is it.
    options.max_retries = 1;
    let attempt = async {
        let fetcher = Fetcher::with_private_network(url, &options, cancel, rule).await?;
        fetcher.get(url, Some(0..1), 1).await
    };
    match tokio::time::timeout(TEST_TIMEOUT, attempt).await {
        Err(_) => ProxyTestResult::TimedOut,
        Ok(Ok(_)) => ProxyTestResult::Works,
        Ok(Err(error)) => match error {
            // An answer came back through the proxy, whatever it was.
            EngineError::HttpStatus { .. }
            | EngineError::TooLarge { .. }
            | EngineError::RangeNotHonored { .. } => ProxyTestResult::Works,
            EngineError::ProxyLoginRefused => ProxyTestResult::LoginRefused,
            EngineError::Network(e) if e.is_timeout() => ProxyTestResult::TimedOut,
            other => ProxyTestResult::Unreachable {
                reason: other.to_string(),
            },
        },
    }
}

#[cfg(test)]
mod tests;
