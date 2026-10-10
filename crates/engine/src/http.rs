//! HTTP requests: client setup, the first request, and ranged requests.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION,
    CONTENT_RANGE, CONTENT_TYPE, DNT, ETAG, HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED,
    LOCATION, PRAGMA, RANGE, REFERER, RETRY_AFTER, USER_AGENT,
};
use reqwest::{Client, Response, StatusCode, redirect};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use url::Url;

use crate::download::DownloadOptions;
use crate::error::{Error, Result};
use crate::naming;
use crate::segment::UNKNOWN_END;

/// Most redirects followed for one request, the same limit browsers use.
const MAX_REDIRECTS: usize = 20;

/// Longest `Retry-After` the engine waits for; a longer one is cut to this.
pub(crate) const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);

/// Headers that carry no credentials, so they may follow a redirect to another
/// origin. Everything else the caller supplied (cookies, authorization, API keys,
/// any custom header) is only ever sent to the origin it was given for.
/// `Referer` is cut down to its origin first, as browsers do.
const PORTABLE_HEADERS: [HeaderName; 7] = [
    ACCEPT,
    ACCEPT_LANGUAGE,
    CACHE_CONTROL,
    DNT,
    PRAGMA,
    REFERER,
    USER_AGENT,
];

/// Values that change when the file on the server changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validators {
    fn from_headers(headers: &HeaderMap) -> Self {
        let text = |name| {
            headers
                .get(name)
                .and_then(|v: &HeaderValue| v.to_str().ok())
                .map(str::to_string)
        };
        Self {
            etag: text(ETAG),
            last_modified: text(LAST_MODIFIED),
        }
    }
}

/// A failed request, with how long the server asked us to wait before trying
/// again (`Retry-After` on a 429 or 503), if it said.
#[derive(Debug)]
pub(crate) struct Failure {
    pub error: Error,
    pub retry_after: Option<Duration>,
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }
}

impl From<reqwest::Error> for Failure {
    fn from(error: reqwest::Error) -> Self {
        Error::network(error).into()
    }
}

impl Failure {
    /// A response whose status is not the one wanted.
    pub(crate) fn status(response: &Response) -> Self {
        let status = response.status();
        let retry_after = matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        )
        .then(|| response.headers().get(RETRY_AFTER))
        .flatten()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_retry_after(v, SystemTime::now()));
        Self {
            error: Error::HttpStatus {
                status: status.as_u16(),
                url: response.url().to_string(),
            },
            retry_after,
        }
    }
}

/// Reads a `Retry-After` value: either a number of seconds or an HTTP date.
/// A date in the past means "now". The result is capped at [`MAX_RETRY_AFTER`].
pub(crate) fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    let delay = if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        // Too many digits for u64 is still a valid (huge) delay.
        Duration::from_secs(value.parse().unwrap_or(u64::MAX))
    } else {
        let at = httpdate::parse_http_date(value).ok()?;
        at.duration_since(now).unwrap_or(Duration::ZERO)
    };
    Some(delay.min(MAX_RETRY_AFTER))
}

/// The caller's request headers, split by where they may be sent.
///
/// Credentials belong to the origin (scheme, host and port) of the URL the
/// caller asked for. If that URL redirects elsewhere, the new origin only gets
/// the [`PORTABLE_HEADERS`]; this holds for the first request and for every
/// worker request that goes straight to the redirected URL afterwards.
#[derive(Debug)]
pub(crate) struct RequestHeaders {
    scheme: String,
    host: Option<String>,
    port: Option<u16>,
    all: HeaderMap,
    portable: HeaderMap,
}

impl RequestHeaders {
    pub fn new(origin: &Url, headers: &HeaderMap) -> Self {
        let mut portable = HeaderMap::new();
        for (name, value) in headers {
            if *name == REFERER {
                if let Some(origin_only) = referer_origin(value) {
                    portable.append(REFERER, origin_only);
                }
            } else if PORTABLE_HEADERS.contains(name) {
                portable.append(name.clone(), value.clone());
            }
        }
        Self {
            scheme: origin.scheme().to_string(),
            host: origin.host_str().map(str::to_string),
            port: origin.port_or_known_default(),
            all: headers.clone(),
            portable,
        }
    }

    fn same_origin(&self, url: &Url) -> bool {
        url.scheme() == self.scheme
            && url.host_str() == self.host.as_deref()
            && url.port_or_known_default() == self.port
    }

    /// The headers a request to `url` may carry.
    pub fn for_url(&self, url: &Url) -> &HeaderMap {
        if self.same_origin(url) {
            &self.all
        } else {
            &self.portable
        }
    }
}

/// `https://site.example/page?x` becomes `https://site.example/`, the form
/// browsers send to other origins. Anything unparsable is dropped.
fn referer_origin(value: &HeaderValue) -> Option<HeaderValue> {
    let url = Url::parse(value.to_str().ok()?).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    HeaderValue::from_str(&format!("{}/", url.origin().ascii_serialization())).ok()
}

/// Moves `user:password@` out of the URL into an `Authorization: Basic` header,
/// so it is sent on every request to that origin (not only the first, as the
/// HTTP client would) and never to another origin. A caller's own
/// `Authorization` header wins.
fn split_userinfo(url: &Url, headers: &mut HeaderMap) -> Url {
    if url.username().is_empty() && url.password().is_none() {
        return url.clone();
    }
    let user = percent_decode_str(url.username()).decode_utf8_lossy();
    let password = url
        .password()
        .map(|p| percent_decode_str(p).decode_utf8_lossy())
        .unwrap_or_default();
    if !headers.contains_key(AUTHORIZATION) {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
        if let Ok(mut value) = HeaderValue::from_str(&format!("Basic {token}")) {
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
    }
    let mut clean = url.clone();
    // Only fails for URLs that cannot have credentials, which this one has.
    let _ = clean.set_username("");
    let _ = clean.set_password(None);
    clean
}

/// What every request of one download shares: which headers may go where,
/// whether to follow redirects from `https` to `http`, how long to wait for an
/// answer, and the token that cancels the download.
#[derive(Debug)]
pub(crate) struct Session {
    pub headers: RequestHeaders,
    allow_insecure_redirects: bool,
    answer_timeout: Duration,
    cancel: CancellationToken,
    /// Checked before every request and every redirect hop; see [`Guard`].
    pub guard: Option<Guard>,
}

/// The future a [`Guard`] returns.
pub(crate) type GuardCheck =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>>;

/// Decides whether a request may go to an address; an error stops it before
/// anything is sent. Used by [`Fetcher`](crate::Fetcher) to keep streams away
/// from the local network.
#[derive(Clone)]
pub(crate) struct Guard(pub Arc<dyn Fn(&Url) -> GuardCheck + Send + Sync>);

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Guard")
    }
}

impl Session {
    /// Returns the session and the URL to request (credentials moved out of it).
    pub fn new(url: &Url, opts: &DownloadOptions, cancel: CancellationToken) -> (Self, Url) {
        let mut headers = opts.headers.clone();
        let url = split_userinfo(url, &mut headers);
        let session = Self {
            headers: RequestHeaders::new(&url, &headers),
            allow_insecure_redirects: opts.allow_insecure_redirects,
            answer_timeout: opts.stall_timeout,
            cancel,
            guard: None,
        };
        (session, url)
    }
}

/// Builds the HTTP clients of one download.
///
/// Each worker gets its own client, so each worker gets its own TCP connection:
/// HTTP/2 would otherwise multiplex every range request over one connection and
/// one congestion window, which removes the point of splitting. All clients
/// share one rustls configuration, and with it one TLS session cache, so once
/// a connection to a host has done a full handshake the others resume the
/// session (no certificate exchange or verification, and one round trip less on
/// TLS 1.2).
pub(crate) struct Clients {
    tls: rustls::ClientConfig,
    user_agent: String,
    connect_timeout: Duration,
}

/// The process-wide TLS configuration: the system's root certificates are read
/// once, and sessions resume across downloads from the same host too.
static SHARED_TLS: OnceLock<rustls::ClientConfig> = OnceLock::new();

impl Clients {
    /// Uses the operating system's root certificates, read once per process.
    /// The first call reads the certificate store, so it must not run on an
    /// async thread; see [`Clients::shared_ready`].
    pub fn new(opts: &DownloadOptions) -> Self {
        let tls = SHARED_TLS.get_or_init(|| tls_config(native_roots()));
        Self::with_config(opts, tls.clone())
    }

    /// Whether [`Clients::new`] would return without touching the disk.
    pub fn shared_ready() -> bool {
        SHARED_TLS.get().is_some()
    }

    /// Uses the given root certificates and a session cache of its own.
    #[cfg(test)]
    pub fn with_roots(opts: &DownloadOptions, roots: rustls::RootCertStore) -> Self {
        Self::with_config(opts, tls_config(roots))
    }

    fn with_config(opts: &DownloadOptions, tls: rustls::ClientConfig) -> Self {
        Self {
            tls,
            user_agent: opts.user_agent.clone(),
            connect_timeout: opts.connect_timeout,
        }
    }

    /// A client with its own connection pool and the shared TLS session cache.
    pub fn build(&self) -> Result<Client> {
        self.builder().build().map_err(Error::network)
    }

    /// The settings every client shares, for a caller that adds its own.
    pub fn builder(&self) -> reqwest::ClientBuilder {
        let mut headers = HeaderMap::new();
        // Compressed transfer encodings break byte offsets.
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        Client::builder()
            // A clone shares the session cache: it lives behind an `Arc` in the config.
            .use_preconfigured_tls(self.tls.clone())
            .user_agent(self.user_agent.as_str())
            .default_headers(headers)
            // Redirects are followed by `send`, which decides per hop which
            // caller headers may go along.
            .redirect(redirect::Policy::none())
            .connect_timeout(self.connect_timeout)
            .tcp_nodelay(true)
            .pool_max_idle_per_host(1)
    }
}

/// The same TLS setup reqwest builds by default (TLS 1.2 and 1.3, the process's
/// crypto provider or else ring, HTTP/2 offered through ALPN), built once.
fn tls_config(roots: rustls::RootCertStore) -> rustls::ClientConfig {
    let installed = rustls::crypto::CryptoProvider::get_default().cloned();
    let builder = installed
        .and_then(|provider| {
            rustls::ClientConfig::builder_with_provider(provider)
                .with_protocol_versions(rustls::ALL_VERSIONS)
                .ok()
        })
        .unwrap_or_else(|| {
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_protocol_versions(rustls::ALL_VERSIONS)
            .expect("the ring provider supports TLS 1.2 and 1.3")
        });
    let mut config = builder.with_root_certificates(roots).with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    config
}

/// The operating system's trusted root certificates. Unreadable entries are
/// skipped; with none at all, HTTPS requests fail with a certificate error.
fn native_roots() -> rustls::RootCertStore {
    let loaded = rustls_native_certs::load_native_certs();
    for error in &loaded.errors {
        debug!(%error, "skipped part of the system certificate store");
    }
    let mut roots = rustls::RootCertStore::empty();
    let (added, ignored) = roots.add_parsable_certificates(loaded.certs);
    if added == 0 {
        warn!(
            ignored,
            "no usable root certificates found; HTTPS downloads will fail"
        );
    }
    roots
}

/// Sends `GET url`, following redirects. Each hop carries only the caller
/// headers its origin may see (see [`RequestHeaders`]), refuses to go from
/// `https` to `http` unless allowed, gives up waiting for an answer after the
/// stall timeout, and returns [`Error::Cancelled`] as soon as the download is
/// cancelled.
pub(crate) async fn send(
    client: &Client,
    url: &Url,
    session: &Session,
    range: Option<&str>,
) -> Result<Response, Failure> {
    let asked = url;
    let mut url = url.clone();
    let mut hops = 0;
    loop {
        if let Some(guard) = &session.guard {
            (guard.0)(&url).await?;
        }
        let mut request = client
            .get(url.clone())
            .headers(session.headers.for_url(&url).clone());
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        let response = tokio::select! {
            biased;
            _ = session.cancel.cancelled() => return Err(Error::Cancelled.into()),
            answer = tokio::time::timeout(session.answer_timeout, request.send()) => match answer {
                Ok(response) => response?,
                Err(_) => return Err(Error::Stalled(session.answer_timeout.as_secs()).into()),
            },
        };
        let status = response.status();
        if !matches!(
            status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            return Ok(response);
        }
        let Some(location) = response.headers().get(LOCATION) else {
            return Err(Error::RedirectWithoutLocation {
                status: status.as_u16(),
                url: url.to_string(),
            }
            .into());
        };
        if hops == MAX_REDIRECTS {
            return Err(Error::TooManyRedirects {
                url: asked.to_string(),
            }
            .into());
        }
        hops += 1;
        let next = location
            .to_str()
            .ok()
            .and_then(|l| url.join(l).ok())
            .filter(|next| matches!(next.scheme(), "http" | "https"))
            .ok_or_else(|| {
                Error::InvalidUrl(format!(
                    "{url} redirects to {location:?}, which is not an http or https address"
                ))
            })?;
        if url.scheme() == "https" && next.scheme() == "http" && !session.allow_insecure_redirects {
            return Err(Error::InsecureRedirect {
                from: url.to_string(),
                to: next.to_string(),
            }
            .into());
        }
        debug!(from = %url, to = %next, "following redirect");
        url = next;
    }
}

/// What the first response says about the file.
pub struct Probe {
    pub response: Response,
    /// URL after redirects; later requests go straight here.
    pub final_url: Url,
    pub ranged: bool,
    pub total: Option<u64>,
    pub validators: Validators,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
}

/// Sends `GET` with `Range: bytes=0-`. A `206` proves ranges work and gives the
/// size, and its body is already the start of the file, so no extra round trip
/// is spent on a separate `HEAD`.
pub(crate) async fn probe(client: &Client, url: &Url, session: &Session) -> Result<Probe, Failure> {
    let response = send(client, url, session, Some("bytes=0-")).await?;
    let status = response.status();
    let final_url = response.url().clone();
    let response_headers = response.headers();
    let validators = Validators::from_headers(response_headers);
    let file_name = response_headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(naming::from_content_disposition);
    let content_type = response_headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let (ranged, total) = match status {
        StatusCode::PARTIAL_CONTENT => match content_range(response_headers) {
            Some(ContentRange {
                start: 0,
                total: Some(total),
                ..
            }) => (true, Some(total)),
            Some(ContentRange {
                start: 0,
                total: None,
                ..
            }) => (false, None),
            other => {
                return Err(Error::RangeNotHonored {
                    expected: 0,
                    got: other.map(|c| c.start),
                }
                .into());
            }
        },
        StatusCode::OK => (false, response.content_length()),
        // `bytes=0-` on an empty file.
        StatusCode::RANGE_NOT_SATISFIABLE
            if content_range(response_headers).and_then(|c| c.total) == Some(0) =>
        {
            (false, Some(0))
        }
        _ => return Err(Failure::status(&response)),
    };

    Ok(Probe {
        response,
        final_url,
        ranged,
        total,
        validators,
        file_name,
        content_type,
    })
}

/// Requests `[from, to)`. Without range support the whole body is requested and
/// the first `from` bytes must be skipped; the returned number says how many.
pub(crate) async fn fetch(
    client: &Client,
    url: &Url,
    session: &Session,
    from: u64,
    to: u64,
    ranged: bool,
    expected_total: Option<u64>,
) -> Result<(Response, u64), Failure> {
    if !ranged {
        let response = send(client, url, session, None).await?;
        check_status(&response, StatusCode::OK)?;
        return Ok((response, from));
    }

    let range = if to == UNKNOWN_END {
        format!("bytes={from}-")
    } else {
        format!("bytes={from}-{}", to - 1)
    };
    let response = send(client, url, session, Some(&range)).await?;
    if response.status() == StatusCode::OK {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: None,
        }
        .into());
    }
    check_status(&response, StatusCode::PARTIAL_CONTENT)?;
    let got = content_range(response.headers());
    if got.map(|c| c.start) != Some(from) {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: got.map(|c| c.start),
        }
        .into());
    }
    if let (Some(expected), Some(actual)) = (expected_total, got.and_then(|c| c.total))
        && expected != actual
    {
        return Err(Error::RemoteChanged.into());
    }
    Ok((response, 0))
}

fn check_status(response: &Response, want: StatusCode) -> Result<(), Failure> {
    if response.status() == want {
        Ok(())
    } else {
        Err(Failure::status(response))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange {
    pub start: u64,
    pub end_inclusive: u64,
    pub total: Option<u64>,
}

pub(crate) fn content_range(headers: &HeaderMap) -> Option<ContentRange> {
    parse_content_range(headers.get(CONTENT_RANGE)?.to_str().ok()?)
}

/// Parses `bytes 0-99/1000`, `bytes 0-99/*` and `bytes */1000`.
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        t => Some(t.parse().ok()?),
    };
    if span.trim() == "*" {
        return Some(ContentRange {
            start: 0,
            end_inclusive: 0,
            total,
        });
    }
    let (start, end) = span.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end_inclusive = end.trim().parse().ok()?;
    (start <= end_inclusive).then_some(ContentRange {
        start,
        end_inclusive,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{AUTHORIZATION, COOKIE};

    #[test]
    fn content_range_forms() {
        assert_eq!(
            parse_content_range("bytes 0-99/1000"),
            Some(ContentRange {
                start: 0,
                end_inclusive: 99,
                total: Some(1000)
            })
        );
        assert_eq!(parse_content_range("bytes 5-9/*").unwrap().total, None);
        assert_eq!(parse_content_range("bytes */0").unwrap().total, Some(0));
        assert_eq!(parse_content_range("bytes 9-5/10"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }

    #[test]
    fn retry_after_forms() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(parse_retry_after("7", now), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
        let later = httpdate::fmt_http_date(now + Duration::from_secs(42));
        assert_eq!(
            parse_retry_after(&later, now),
            Some(Duration::from_secs(42))
        );
        // The two obsolete date formats HTTP still allows.
        let base = httpdate::fmt_http_date(now);
        assert_eq!(base, "Tue, 14 Nov 2023 22:13:20 GMT");
        assert_eq!(
            parse_retry_after("Tuesday, 14-Nov-23 22:13:30 GMT", now),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            parse_retry_after("Tue Nov 14 22:13:25 2023", now),
            Some(Duration::from_secs(5))
        );
        let earlier = httpdate::fmt_http_date(now - Duration::from_secs(60));
        assert_eq!(parse_retry_after(&earlier, now), Some(Duration::ZERO));
        assert_eq!(parse_retry_after("86400", now), Some(MAX_RETRY_AFTER));
        assert_eq!(
            parse_retry_after("99999999999999999999999", now),
            Some(MAX_RETRY_AFTER)
        );
        assert_eq!(parse_retry_after("-5", now), None);
        assert_eq!(parse_retry_after("soon", now), None);
        assert_eq!(parse_retry_after("", now), None);
    }

    #[test]
    fn credentials_only_go_to_their_own_origin() {
        let mut given = HeaderMap::new();
        given.insert(COOKIE, HeaderValue::from_static("session=secret"));
        given.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        given.insert("x-api-key", HeaderValue::from_static("secret"));
        given.insert(
            REFERER,
            HeaderValue::from_static("https://site.example/page"),
        );
        given.insert(USER_AGENT, HeaderValue::from_static("Browser/1.0"));
        let origin = Url::parse("https://files.example/a.bin").unwrap();
        let headers = RequestHeaders::new(&origin, &given);

        let url = |s: &str| Url::parse(s).unwrap();
        // Same origin, including the default port written out.
        assert_eq!(headers.for_url(&url("https://files.example/b.bin")), &given);
        assert_eq!(
            headers.for_url(&url("https://files.example:443/b.bin")),
            &given
        );

        for other in [
            "https://cdn.example/a.bin",        // other host
            "https://files.example:8443/a.bin", // other port
            "http://files.example/a.bin",       // other scheme (downgrade)
            "https://sub.files.example/a.bin",  // subdomain
        ] {
            let sent = headers.for_url(&url(other));
            assert!(sent.get(COOKIE).is_none(), "cookie leaked to {other}");
            assert!(sent.get(AUTHORIZATION).is_none(), "auth leaked to {other}");
            assert!(sent.get("x-api-key").is_none(), "key leaked to {other}");
            // Other origins only learn which site the link came from.
            assert_eq!(sent.get(REFERER).unwrap(), "https://site.example/");
            assert_eq!(sent.get(USER_AGENT), given.get(USER_AGENT));
        }
    }

    #[test]
    fn referer_is_cut_to_its_origin_for_other_origins() {
        let cut = |v: &'static str| referer_origin(&HeaderValue::from_static(v));
        assert_eq!(
            cut("http://site.example:8080/a/b?token=1#x").unwrap(),
            "http://site.example:8080/"
        );
        assert_eq!(
            cut("https://site.example:443/a").unwrap(),
            "https://site.example/"
        );
        assert_eq!(cut("not a url"), None);
        assert_eq!(cut("file:///etc/passwd"), None);
    }

    #[test]
    fn url_credentials_become_an_origin_bound_header() {
        let mut headers = HeaderMap::new();
        let url = Url::parse("https://us%40er:p%3Ass@files.example/a.bin").unwrap();
        let clean = split_userinfo(&url, &mut headers);
        assert_eq!(clean.as_str(), "https://files.example/a.bin");
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("us@er:p:ss")
        );
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), expected.as_str());

        // A header the caller set wins over the URL.
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer mine"));
        split_userinfo(&url, &mut headers);
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer mine");

        // No credentials, nothing added.
        let mut headers = HeaderMap::new();
        let plain = Url::parse("https://files.example/a.bin").unwrap();
        assert_eq!(split_userinfo(&plain, &mut headers), plain);
        assert!(headers.is_empty());
    }

    /// Against a local TLS server: session resumption, checked from the server
    /// side (it records whether each handshake was full or resumed), and
    /// redirects from https to http.
    mod tls {
        use super::*;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use rustls::HandshakeKind;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        struct TlsServer {
            url: Url,
            roots: rustls::RootCertStore,
            handshakes: Arc<Mutex<Vec<HandshakeKind>>>,
        }

        const OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

        /// Answers every request with a tiny body and closes the connection, so
        /// each request needs a new TCP connection and a new TLS handshake.
        async fn start_tls_server() -> TlsServer {
            start_tls_server_answering(OK.to_vec()).await
        }

        /// Reads one request's headers from `stream`; false if it closed first.
        async fn read_request(stream: &mut (impl AsyncReadExt + Unpin)) -> bool {
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => return false,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            true
        }

        /// Answers every request with `response` and closes the connection.
        async fn start_tls_server_answering(response: Vec<u8>) -> TlsServer {
            let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
            let cert_der = CertificateDer::from(cert.cert.der().to_vec());
            let key =
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));
            let mut roots = rustls::RootCertStore::empty();
            roots.add(cert_der.clone()).unwrap();

            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ServerConfig::builder_with_provider(provider)
                .with_protocol_versions(rustls::ALL_VERSIONS)
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![cert_der], key)
                .unwrap();
            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let handshakes = Arc::new(Mutex::new(Vec::new()));
            {
                let handshakes = handshakes.clone();
                tokio::spawn(async move {
                    loop {
                        let (tcp, _) = listener.accept().await.unwrap();
                        let acceptor = acceptor.clone();
                        let handshakes = handshakes.clone();
                        let response = response.clone();
                        tokio::spawn(async move {
                            let Ok(mut tls) = acceptor.accept(tcp).await else {
                                return;
                            };
                            if let Some(kind) = tls.get_ref().1.handshake_kind() {
                                handshakes.lock().unwrap().push(kind);
                            }
                            if !read_request(&mut tls).await {
                                return;
                            }
                            let _ = tls.write_all(&response).await;
                            let _ = tls.shutdown().await;
                        });
                    }
                });
            }
            TlsServer {
                url: Url::parse(&format!("https://{addr}/file.bin")).unwrap(),
                roots,
                handshakes,
            }
        }

        async fn get(client: &Client, url: &Url) {
            let body = client.get(url.clone()).send().await.unwrap();
            assert_eq!(body.bytes().await.unwrap(), "ok");
        }

        #[tokio::test]
        async fn connections_of_one_download_resume_the_first_session() {
            let server = start_tls_server().await;
            let opts = DownloadOptions::default();
            let clients = Clients::with_roots(&opts, server.roots.clone());

            // One client per worker, exactly as `download` builds them.
            for _ in 0..4 {
                get(&clients.build().unwrap(), &server.url).await;
            }

            let kinds = server.handshakes.lock().unwrap().clone();
            assert_eq!(
                kinds,
                vec![
                    HandshakeKind::Full,
                    HandshakeKind::Resumed,
                    HandshakeKind::Resumed,
                    HandshakeKind::Resumed
                ],
                "4 connections, only the first with a full handshake"
            );
        }

        /// A plain HTTP server answering `OK`; returns its URL and request count.
        async fn start_plain_server() -> (Url, Arc<AtomicUsize>) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let requests = Arc::new(AtomicUsize::new(0));
            let counter = requests.clone();
            tokio::spawn(async move {
                loop {
                    let (mut tcp, _) = listener.accept().await.unwrap();
                    let counter = counter.clone();
                    tokio::spawn(async move {
                        if read_request(&mut tcp).await {
                            counter.fetch_add(1, Ordering::SeqCst);
                            let _ = tcp.write_all(OK).await;
                            let _ = tcp.shutdown().await;
                        }
                    });
                }
            });
            let url = Url::parse(&format!("http://{addr}/file.bin")).unwrap();
            (url, requests)
        }

        #[tokio::test]
        async fn https_to_http_redirects_are_refused_unless_allowed() {
            let (plain, plain_requests) = start_plain_server().await;
            let redirect = format!(
                "HTTP/1.1 302 Found\r\nLocation: {plain}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let server = start_tls_server_answering(redirect.into_bytes()).await;
            let mut opts = DownloadOptions::default();
            let client = Clients::with_roots(&opts, server.roots.clone())
                .build()
                .unwrap();

            // Refused by default, before anything is sent over plain HTTP.
            let (session, url) = Session::new(&server.url, &opts, CancellationToken::new());
            let failure = send(&client, &url, &session, None).await.unwrap_err();
            match failure.error {
                Error::InsecureRedirect { from, to } => {
                    assert_eq!(from, server.url.as_str());
                    assert_eq!(to, plain.as_str());
                }
                other => panic!("expected InsecureRedirect, got {other:?}"),
            }
            assert_eq!(plain_requests.load(Ordering::SeqCst), 0);

            // Followed when the caller allows it.
            opts.allow_insecure_redirects = true;
            let (session, url) = Session::new(&server.url, &opts, CancellationToken::new());
            let response = send(&client, &url, &session, None).await.unwrap();
            assert_eq!(response.url(), &plain);
            assert_eq!(response.bytes().await.unwrap(), "ok");
            assert_eq!(plain_requests.load(Ordering::SeqCst), 1);
        }

        #[tokio::test]
        async fn separate_tls_configs_cannot_resume() {
            // Control: proves the test above detects the difference.
            let server = start_tls_server().await;
            let opts = DownloadOptions::default();
            for _ in 0..3 {
                let clients = Clients::with_roots(&opts, server.roots.clone());
                get(&clients.build().unwrap(), &server.url).await;
            }
            let kinds = server.handshakes.lock().unwrap().clone();
            assert_eq!(kinds, vec![HandshakeKind::Full; 3]);
        }
    }
}
