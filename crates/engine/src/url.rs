//! The links the engine downloads, and taking sign-in details out of a URL
//! before it is shown, logged, stored or passed on.
//!
//! A link can carry a user name and password (`https://alice:secret@host/`).
//! Where it goes after the request that needs them, they must not follow.

use ::url::Url;

/// The web schemes the engine downloads.
pub const WEB_SCHEMES: [&str; 2] = ["http", "https"];

/// The FTP schemes the engine downloads: plain, with implicit TLS (`ftps`) and with explicit TLS (`ftpes`).
pub const FTP_SCHEMES: [&str; 3] = ["ftp", "ftps", "ftpes"];

/// Whether the engine downloads links of `url`'s scheme.
#[must_use]
pub fn is_supported(url: &Url) -> bool {
    WEB_SCHEMES.contains(&url.scheme()) || is_ftp(url)
}

/// Whether `url` is an FTP link the engine downloads.
#[must_use]
pub fn is_ftp(url: &Url) -> bool {
    FTP_SCHEMES.contains(&url.scheme())
}

/// `url` without its `user:password@`, for passing on, logging or storing where
/// credentials must not go. A URL that cannot carry credentials has none and
/// comes back as it is.
///
/// ```
/// use wirecatch_engine::Url;
/// use wirecatch_engine::url::without_credentials;
///
/// let url = Url::parse("https://alice:secret@example.com/file.zip").unwrap();
/// assert_eq!(without_credentials(&url).as_str(), "https://example.com/file.zip");
/// ```
#[must_use]
pub fn without_credentials(url: &Url) -> Url {
    let mut clean = without_password(url);
    removed(clean.set_username(""));
    clean
}

/// `url` without its password, keeping the user name: an FTP link shown or
/// saved this way still says which account it signs in with, which tells two
/// downloads from the same server apart. [`without_credentials`] removes both.
#[must_use]
pub(crate) fn without_password(url: &Url) -> Url {
    let mut clean = url.clone();
    removed(clean.set_password(None));
    clean
}

/// Takes the outcome of removing a user name or password from a URL. It is
/// refused only for a URL that cannot carry credentials, which has none, so
/// either way none are left.
fn removed(outcome: Result<(), ()>) {
    let (Ok(()) | Err(())) = outcome;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_and_ftp_links_are_downloaded_and_nothing_else() {
        let link = |raw: &str| Url::parse(raw).unwrap();
        for raw in [
            "http://h/f",
            "https://h/f",
            "ftp://h/f",
            "ftps://h/f",
            "ftpes://h/f",
        ] {
            assert!(is_supported(&link(raw)), "{raw}");
        }
        for raw in ["file:///f", "sftp://h/f", "data:,x", "magnet:?xt=x"] {
            assert!(!is_supported(&link(raw)), "{raw}");
        }
        assert!(is_ftp(&link("ftpes://h/f")));
        assert!(!is_ftp(&link("https://h/f")));
    }

    #[test]
    fn without_password_keeps_the_user_name() {
        let url = Url::parse("ftp://alice:secret@example.com/file.zip").unwrap();
        assert_eq!(
            without_password(&url).as_str(),
            "ftp://alice@example.com/file.zip"
        );
    }
}
