//! The app's proxy setting (D71): which proxy everything the app sends goes through. The
//! windows see it in [`AppSettings::proxy`](crate::app::AppSettings::proxy) without its
//! passwords, which never leave the app, and change it as a whole with a [`ProxyChange`],
//! after testing it with a [`ProxyTest`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Secret;
use crate::app::{Update, known_or_default};

crate::exported::exported_consts! {
    /// Most characters in a proxy's host name.
    pub const MAX_PROXY_HOST_CHARS: usize = 253;
    /// Most bytes in a proxy's user name or password: SOCKS5 sends each in at most 255 (RFC 1929).
    pub const MAX_PROXY_LOGIN_BYTES: usize = 255;
    /// Most hosts the proxy's exceptions list.
    pub const MAX_PROXY_EXCEPTIONS: usize = 256;
}

/// Where the app's requests go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProxyMode {
    /// The computer's proxy: the proxy variables of the environment, or else on Windows
    /// the proxy in the network settings. The default.
    #[default]
    System,
    /// No proxy: everything goes directly.
    Direct,
    /// The proxies the person entered.
    Manual,
}

/// What a manual proxy speaks.
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

/// Which of the manual proxies a password belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProxyRole {
    /// The one proxy for every protocol.
    All,
    /// The proxy for `http` links.
    Http,
    /// The proxy for `https` links.
    Https,
    /// The proxy for FTP links.
    Ftp,
    /// The SOCKS proxy for every protocol without a proxy of its own.
    Socks,
}

impl ProxyRole {
    /// Every role, in the order the settings show them.
    pub const ALL: [Self; 5] = [Self::All, Self::Http, Self::Https, Self::Ftp, Self::Socks];
}

/// One manual proxy. It is set when its host is not empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyServer {
    /// What it speaks. The SOCKS proxy is SOCKS5 whatever this says.
    #[serde(default, with = "known_or_default")]
    #[ts(as = "ProxyKind")]
    pub kind: ProxyKind,
    /// Its host name or address; empty when it is not set.
    pub host: String,
    /// Its port.
    pub port: u16,
    /// The user name it asks for; empty for none.
    pub user: String,
    /// Whether a password is saved for it. The password itself stays in the app (D71.2).
    pub has_password: bool,
}

impl ProxyServer {
    /// Whether a proxy is entered here.
    #[must_use]
    pub fn is_set(&self) -> bool {
        !self.host.trim().is_empty()
    }
}

/// The app's proxy setting, as the windows see it: without passwords.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxySettings {
    /// Where requests go.
    #[serde(default, with = "known_or_default")]
    #[ts(as = "ProxyMode")]
    pub mode: ProxyMode,
    /// Manual: a proxy for each protocol (`http`, `https`, `ftp`, `socks`) rather than one
    /// for every protocol (`all`). Both sets are kept, so switching back loses nothing.
    pub per_protocol: bool,
    /// The proxy for every protocol.
    pub all: ProxyServer,
    /// The proxy for `http` links.
    pub http: ProxyServer,
    /// The proxy for `https` links.
    pub https: ProxyServer,
    /// The proxy for FTP links.
    pub ftp: ProxyServer,
    /// The SOCKS5 proxy for every protocol without a proxy of its own.
    pub socks: ProxyServer,
    /// Hosts reached directly with a manual proxy: names (each covers the names under it,
    /// and may be written `*.corp.example`), addresses, networks (`10.0.0.0/8` or `10.*`),
    /// and `<local>` for every name without a dot.
    pub exceptions: Vec<String>,
    /// With a manual proxy that cannot carry FTP (anything but SOCKS5), FTP links go
    /// directly instead of failing (D71.4).
    pub ftp_direct: bool,
}

impl ProxySettings {
    /// The proxy entered for `role`.
    #[must_use]
    pub fn server(&self, role: ProxyRole) -> &ProxyServer {
        match role {
            ProxyRole::All => &self.all,
            ProxyRole::Http => &self.http,
            ProxyRole::Https => &self.https,
            ProxyRole::Ftp => &self.ftp,
            ProxyRole::Socks => &self.socks,
        }
    }

    /// The proxy entered for `role`, to change.
    pub fn server_mut(&mut self, role: ProxyRole) -> &mut ProxyServer {
        match role {
            ProxyRole::All => &mut self.all,
            ProxyRole::Http => &mut self.http,
            ProxyRole::Https => &mut self.https,
            ProxyRole::Ftp => &mut self.ftp,
            ProxyRole::Socks => &mut self.socks,
        }
    }

    /// The roles whose proxies are in use: none outside manual mode, `all` with one proxy
    /// for every protocol, and the others with a proxy each.
    #[must_use]
    pub fn roles_in_use(&self) -> &'static [ProxyRole] {
        match (self.mode, self.per_protocol) {
            (ProxyMode::Manual, false) => &[ProxyRole::All],
            (ProxyMode::Manual, true) => &[
                ProxyRole::Http,
                ProxyRole::Https,
                ProxyRole::Ftp,
                ProxyRole::Socks,
            ],
            _ => &[],
        }
    }
}

/// New passwords for the manual proxies: a missing field keeps the saved one, `null` removes
/// it, and a string replaces it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct ProxyPasswords {
    /// For [`ProxyRole::All`].
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<String>>")]
    pub all: Update<Option<Secret>>,
    /// For [`ProxyRole::Http`].
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<String>>")]
    pub http: Update<Option<Secret>>,
    /// For [`ProxyRole::Https`].
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<String>>")]
    pub https: Update<Option<Secret>>,
    /// For [`ProxyRole::Ftp`].
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<String>>")]
    pub ftp: Update<Option<Secret>>,
    /// For [`ProxyRole::Socks`].
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<String>>")]
    pub socks: Update<Option<Secret>>,
}

impl ProxyPasswords {
    /// The change for `role`.
    #[must_use]
    pub fn get(&self, role: ProxyRole) -> &Update<Option<Secret>> {
        match role {
            ProxyRole::All => &self.all,
            ProxyRole::Http => &self.http,
            ProxyRole::Https => &self.https,
            ProxyRole::Ftp => &self.ftp,
            ProxyRole::Socks => &self.socks,
        }
    }
}

/// A new proxy setting, saved as a whole when the person presses Save.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ProxyChange {
    /// The setting; its `has_password` fields are ignored, `passwords` says what changes.
    pub settings: ProxySettings,
    /// The passwords that change.
    pub passwords: ProxyPasswords,
    /// Restart the downloads that are running, so every request follows the new setting at
    /// once; they keep what they downloaded (D71.6). Otherwise they keep the proxy they
    /// started with until they stop.
    pub restart_running: bool,
}

/// A proxy setting to try before it is saved: the same as a [`ProxyChange`] without the
/// restart.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ProxyTest {
    /// The setting to try.
    pub settings: ProxySettings,
    /// Passwords that differ from the saved ones.
    pub passwords: ProxyPasswords,
}

/// What a test of a proxy setting found (D71.5).
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
