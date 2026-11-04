//! The links the engine downloads, and taking sign-in details out of a URL
//! before it is shown, logged, stored or passed on.
//!
//! A link can carry a user name and password (`https://alice:secret@host/`).
//! Where it goes after the request that needs them, they must not follow.

use ::url::{Host, Url};
use percent_encoding::percent_decode_str;

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

/// `url` with its host in the form a web link's has, so that every check that reads the host sees what a connection will reach. The URL parser leaves the host of an `ftps` or `ftpes` link as written, percent-encoded and in mixed case: `ftps://%31%32%37.0.0.1/a` and `ftps://2130706433/a` both connect to `127.0.0.1`. Any other link, and a host that cannot be read, comes back as it is.
///
/// ```
/// use wirecatch_engine::Url;
/// use wirecatch_engine::url::with_normal_host;
///
/// let url = Url::parse("ftps://FILES.Example/a.zip").unwrap();
/// assert_eq!(with_normal_host(url).as_str(), "ftps://files.example/a.zip");
/// ```
#[must_use]
pub fn with_normal_host(url: Url) -> Url {
    let Some(host) = is_ftp(&url)
        .then(|| url.host_str().and_then(|name| read_host(name).ok()))
        .flatten()
    else {
        return url;
    };
    let mut normal = url.clone();
    let set = match host {
        Host::Domain(name) => normal.set_host(Some(&name)).is_ok(),
        Host::Ipv4(ip) => normal.set_ip_host(ip.into()).is_ok(),
        Host::Ipv6(ip) => normal.set_ip_host(ip.into()).is_ok(),
    };
    if set { normal } else { url }
}

/// Why a host written in a link cannot be read.
pub(crate) enum BadHost {
    NotUtf8,
    Unusable,
}

/// Reads `name`, a host as an FTP link holds it, the way a web link's is read.
pub(crate) fn read_host(name: &str) -> Result<Host<String>, BadHost> {
    let decoded = percent_decode_str(name)
        .decode_utf8()
        .map_err(|_| BadHost::NotUtf8)?;
    Host::parse(&decoded).map_err(|_| BadHost::Unusable)
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
pub fn without_password(url: &Url) -> Url {
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
    fn an_ftp_links_host_reads_as_a_web_links_does() {
        for (raw, normal) in [
            ("ftps://%31%32%37.0.0.1/a", "ftps://127.0.0.1/a"),
            ("ftps://2130706433/a", "ftps://127.0.0.1/a"),
            ("ftpes://0x7f.1/a", "ftpes://127.0.0.1/a"),
            (
                "ftps://FILES.Home.Arpa/a.zip",
                "ftps://files.home.arpa/a.zip",
            ),
            (
                "ftpes://u:p@%31%32%37.0.0.1:2121/a;type=i",
                "ftpes://u:p@127.0.0.1:2121/a;type=i",
            ),
            (
                "ftps://b%C3%BCcher.example/a",
                "ftps://xn--bcher-kva.example/a",
            ),
            ("ftps://[::1]/a", "ftps://[::1]/a"),
            // Comes back as it is; connecting reports it.
            ("ftps://%FF/a", "ftps://%FF/a"),
            ("ftp://FILES.example/a", "ftp://files.example/a"),
            ("https://FILES.example/a", "https://files.example/a"),
        ] {
            let url = Url::parse(raw).unwrap();
            assert_eq!(with_normal_host(url).as_str(), normal, "{raw}");
        }
    }

    #[test]
    fn a_normal_host_keeps_its_kind_when_read_back() {
        let url = with_normal_host(Url::parse("ftps://%31%32%37.0.0.1/a").unwrap());
        assert!(matches!(url.host(), Some(Host::Ipv4(_))), "{url}");
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
