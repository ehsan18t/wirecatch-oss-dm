//! Taking sign-in details out of a URL before it is shown, logged, stored or
//! passed on.
//!
//! A link can carry a user name and password (`https://alice:secret@host/`).
//! Where it goes after the request that needs them, they must not follow.

use ::url::Url;

/// `url` without its `user:password@`, for passing on, logging or storing where
/// credentials must not go. A URL that cannot carry credentials has none and
/// comes back as it is. [`without_password`] keeps the user name.
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
    // Fails only for URLs that cannot carry credentials, which then have none.
    let _ = clean.set_username("");
    clean
}

/// `url` without its password, keeping the user name: an FTP link shown or
/// saved this way still says which account it signs in with, which tells two
/// downloads from the same server apart. [`without_credentials`] removes both.
///
/// ```
/// use wirecatch_engine::Url;
/// use wirecatch_engine::url::without_password;
///
/// let url = Url::parse("ftp://alice:secret@example.com/file.zip").unwrap();
/// assert_eq!(without_password(&url).as_str(), "ftp://alice@example.com/file.zip");
/// ```
#[must_use]
pub fn without_password(url: &Url) -> Url {
    let mut clean = url.clone();
    // Fails only for URLs that cannot carry credentials, which then have none.
    let _ = clean.set_password(None);
    clean
}
