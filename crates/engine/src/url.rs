//! Taking sign-in details out of a URL before it is shown, logged, stored or
//! passed on.
//!
//! A link can carry a user name and password (`https://alice:secret@host/`).
//! Where it goes after the request that needs them, they must not follow.

use ::url::Url;

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
    fn without_password_keeps_the_user_name() {
        let url = Url::parse("ftp://alice:secret@example.com/file.zip").unwrap();
        assert_eq!(
            without_password(&url).as_str(),
            "ftp://alice@example.com/file.zip"
        );
    }
}
