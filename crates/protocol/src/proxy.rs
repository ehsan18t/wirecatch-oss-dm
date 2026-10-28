//! Proxies (D73): the proxy profiles the person saves, the app's proxy with the hosts it
//! reaches directly ([`ProxySettings`], in [`AppSettings::proxy`](crate::app::AppSettings::proxy)),
//! and a queue's ([`QueueSettings`](crate::app::QueueSettings)). Passwords never leave the app.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{Update, known_or_default};
use crate::{ProxyProfileId, Secret};

crate::exported::exported_consts! {
    /// Most characters in a proxy's host name.
    pub const MAX_PROXY_HOST_CHARS: usize = 253;
    /// Most bytes in a proxy's user name or password: SOCKS5 sends each in at most 255 (RFC 1929).
    pub const MAX_PROXY_LOGIN_BYTES: usize = 255;
    /// Most hosts in one list of exceptions.
    pub const MAX_PROXY_EXCEPTIONS: usize = 256;
    /// Most characters in a proxy profile's name.
    pub const MAX_PROXY_NAME_CHARS: usize = 60;
    /// Most proxy profiles there can be.
    pub const MAX_PROXY_PROFILES: usize = 100;
}

/// What a proxy speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProxyKind {
    /// An HTTP proxy, reached over plain HTTP.
    #[default]
    Http,
    /// An HTTP proxy reached over TLS.
    Https,
    /// A SOCKS5 proxy, which resolves server names itself, so they never reach this
    /// computer's name server. The only kind that carries FTP.
    Socks5,
}

/// A proxy profile as the person edits it (D73.1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyProfileSettings {
    /// Unique among the profiles; empty shows the server's address instead.
    pub name: String,
    #[serde(default, with = "known_or_default")]
    #[ts(as = "ProxyKind")]
    pub kind: ProxyKind,
    /// Its host name or address.
    pub host: String,
    pub port: u16,
    /// The user name it asks for; empty for none.
    pub user: String,
}

impl ProxyProfileSettings {
    /// Its name, or its server's address when it has none.
    #[must_use]
    pub fn shown_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            self.address()
        } else {
            name.to_string()
        }
    }

    /// `host:port`, with an IPv6 address in brackets.
    #[must_use]
    pub fn address(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// A saved proxy profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyProfile {
    pub id: ProxyProfileId,
    pub settings: ProxyProfileSettings,
    /// Whether a password is saved for it. The password itself stays in the app (D71.2).
    pub has_password: bool,
    /// Its last Test connection, kept apart from its settings; `None` before the first.
    pub last_test: Option<ProxyTestRun>,
}

/// One Test connection of a saved profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyTestRun {
    /// When it ran, in Unix time in milliseconds.
    #[ts(type = "number")]
    pub at: i64,
    pub result: ProxyTestResult,
}

/// Which proxy requests go through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[ts(export)]
pub enum ProxyUse {
    /// The computer's proxy: the proxy variables of the environment, or else on Windows
    /// the proxy in the network settings.
    #[default]
    System,
    /// No proxy: everything goes directly.
    Direct,
    Profile(ProxyProfileId),
}

/// The hosts the app reaches directly (D73.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppExceptions {
    /// The computer's bypass list: `NO_PROXY`, or else on Windows the system proxy's exceptions.
    pub system: bool,
    /// Whether [`hosts`](Self::hosts) are used; unticking keeps the list.
    pub use_hosts: bool,
    /// Names (each covers the names under it, and may be written `*.corp.example`),
    /// addresses, networks (`10.0.0.0/8` or `10.*`), and `<local>` for every name without a dot.
    pub hosts: Vec<String>,
}

impl Default for AppExceptions {
    fn default() -> Self {
        Self {
            system: true,
            use_hosts: true,
            hosts: Vec::new(),
        }
    }
}

/// The hosts a queue reaches directly when it does not follow the app's (D73.7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueExceptions {
    /// The computer's bypass list.
    pub system: bool,
    /// The app's [`hosts`](AppExceptions::hosts).
    pub app_hosts: bool,
    /// Whether [`hosts`](Self::hosts) are used; unticking keeps the list.
    pub use_hosts: bool,
    /// The queue's own hosts, written as [`AppExceptions::hosts`].
    pub hosts: Vec<String>,
}

/// The app's proxy, as [`AppSettings::proxy`](crate::app::AppSettings::proxy) holds it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxySettings {
    /// The proxy of every queue that follows the app.
    pub proxy: ProxyUse,
    pub exceptions: AppExceptions,
    /// An FTP link whose proxy cannot carry FTP (anything but SOCKS5) goes directly
    /// instead of failing (D71.4).
    pub ftp_direct: bool,
}

/// A profile's settings with a change to its password: a missing `password` keeps the saved
/// one, `null` removes it, and a string replaces it. A password kept as it is stays only while
/// the profile's type, server, port and user name stay (D71.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct ProxyProfileDraft {
    pub settings: ProxyProfileSettings,
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<String>>")]
    pub password: Update<Option<Secret>>,
}

/// What a test of a proxy found (D71.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[ts(export)]
pub enum ProxyTestResult {
    /// A request went through and got an answer.
    Works,
    /// The proxy asked for a user name and password, or refused the ones it was given.
    LoginRefused,
    /// The proxy could not be reached, or could not reach the internet.
    Unreachable {
        /// What went wrong, in words.
        reason: String,
    },
    /// Neither the proxy nor the internet answered in time.
    TimedOut,
}
