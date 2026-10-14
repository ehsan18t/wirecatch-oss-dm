//! Values that grant access (cookies, header values, passwords in addresses), kept out of
//! everything that prints: `Debug` output reaches logs, panic messages and failed
//! assertions, which is where a session cookie must never end up (IPB-11).

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::secrets::HIDDEN;

/// A value that grants access, such as a cookie or an `Authorization` header. It crosses
/// the wire as the plain string, but its `Debug` prints `"***"` and it has no
/// `Display`, so reading it takes an explicit [`Secret::expose`].
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps `value`.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for the one place that sends it on.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(HIDDEN, f)
    }
}

/// An address as `Debug` should show it: without the user name and password it may carry
/// (an FTP link often does). Text that is not an address with a host (`me:pw@host` reads
/// as the scheme `me` with no host) is shown only when it cannot hide a password, which is
/// when it has no `@`.
pub(crate) struct Address<'a>(pub(crate) &'a str);

impl fmt::Debug for Address<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match url::Url::parse(self.0) {
            Ok(mut url) if url.has_host() => {
                if url.username().is_empty() && url.password().is_none() {
                    return fmt::Debug::fmt(self.0, f);
                }
                // Neither setter fails on an address with a host; if one ever did, the
                // sign-in would still be there, so nothing of the address prints.
                if url.set_username("").is_err() || url.set_password(None).is_err() {
                    return fmt::Debug::fmt(HIDDEN, f);
                }
                write!(f, "{:?} (sign-in {HIDDEN})", url.as_str())
            }
            _ if self.0.contains('@') => fmt::Debug::fmt(HIDDEN, f),
            _ => fmt::Debug::fmt(self.0, f),
        }
    }
}

/// An address the person gave the app. It may carry a sign-in
/// (`ftp://me:password@host`), which logs and error reports leave out.
// A newtype struct, which serde writes and reads as the string inside it; its `Debug`
// prints it as `Address` does.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(as = "String")]
pub struct Link(String);

impl Link {
    /// The address itself.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for Link {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Link {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&Address(&self.0), f)
    }
}

/// An optional address, printed as [`Address`] is.
pub(crate) struct OptionalAddress<'a>(pub(crate) Option<&'a str>);

impl fmt::Debug for OptionalAddress<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(address) => f.debug_tuple("Some").field(&Address(address)).finish(),
            None => f.write_str("None"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_prints_hidden_and_serializes_as_its_value() {
        let secret = Secret::new("sid=abc");
        assert_eq!(format!("{secret:?}"), r#""***""#);
        assert_eq!(format!("{:?}", Some(secret.clone())), r#"Some("***")"#);
        assert_eq!(serde_json::to_string(&secret).unwrap(), r#""sid=abc""#);
        assert_eq!(
            serde_json::from_str::<Secret>(r#""sid=abc""#).unwrap(),
            secret
        );
        assert_eq!(secret.expose(), "sid=abc");
    }

    #[test]
    fn an_address_prints_without_its_sign_in() {
        let shown = |text: &str| format!("{:?}", Address(text));
        assert_eq!(
            shown("ftp://me:pw@files.example/a.zip"),
            r#""ftp://files.example/a.zip" (sign-in ***)"#
        );
        assert_eq!(
            shown("https://token@example.com/"),
            r#""https://example.com/" (sign-in ***)"#
        );
        assert_eq!(
            shown("https://example.com/a?b=c"),
            r#""https://example.com/a?b=c""#
        );
        assert_eq!(shown("not a url"), r#""not a url""#);
        assert_eq!(shown("me:pw@not a url"), r#""***""#);
        assert_eq!(shown("me:pw@files.example"), r#""***""#);
    }
}
