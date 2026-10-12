//! The FTP reply codes the engine acts on (RFC 959, with `AUTH` from RFC 2228,
//! `EPSV` from RFC 2428 and `SIZE` and `MDTM` from RFC 3659).

/// 120: the server will be ready in a few minutes; its real greeting follows.
pub(crate) const READY_SOON: u16 = 120;
/// 202: the command is not needed here (a login that asks for no password).
pub(crate) const NOT_NEEDED: u16 = 202;
/// 213: the answer to `SIZE` or `MDTM`.
pub(crate) const FILE_STATUS: u16 = 213;
/// 220: the greeting of a server ready for a login.
pub(crate) const SERVICE_READY: u16 = 220;
/// 227: the address of a passive data connection (`PASV`).
pub(crate) const PASSIVE: u16 = 227;
/// 229: the port of an extended passive data connection (`EPSV`).
pub(crate) const EXTENDED_PASSIVE: u16 = 229;
/// 230: logged in.
pub(crate) const LOGGED_IN: u16 = 230;
/// 234: `AUTH TLS` accepted; the TLS handshake follows.
pub(crate) const SECURITY_ACCEPTED: u16 = 234;
/// 331: the user name is known; the password comes next.
pub(crate) const NEED_PASSWORD: u16 = 331;
/// 350: `REST` accepted; the next transfer starts at that offset.
pub(crate) const PENDING: u16 = 350;
/// 421: the service is not available, which is how most servers say they
/// have too many connections.
pub(crate) const SERVICE_UNAVAILABLE: u16 = 421;
/// 425: the server would not open the data connection.
pub(crate) const NO_DATA_CONNECTION: u16 = 425;
/// 530: not logged in, because the login was refused.
pub(crate) const NOT_LOGGED_IN: u16 = 530;
/// 550: the file does not exist or cannot be read.
pub(crate) const FILE_UNAVAILABLE: u16 = 550;

/// 4xx, a "transient negative completion": the command failed but may work
/// when tried again (too many connections, a failed data connection, a busy
/// file).
pub(crate) fn is_transient(code: u16) -> bool {
    (400..500).contains(&code)
}

/// 4xx or 5xx: the command failed.
pub(crate) fn is_failure(code: u16) -> bool {
    (400..600).contains(&code)
}
