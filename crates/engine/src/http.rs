//! HTTP requests: client setup, the first request, and ranged requests.

use std::time::{Duration, SystemTime};

use base64::Engine as _;
use percent_encoding::percent_decode_str;
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, DATE,
    DNT, ETAG, HeaderMap, HeaderName, HeaderValue, LAST_MODIFIED, LOCATION, PRAGMA, RANGE, REFERER,
    RETRY_AFTER, USER_AGENT,
};
use reqwest::{Client, Response, StatusCode};
use tokio_util::sync::CancellationToken;
use tracing::debug;
use url::Url;

use crate::error::{Error, Result};
use crate::naming;
use crate::net_guard::Guard;
use crate::options::DownloadOptions;
use crate::request::RequestContext;

mod client;
mod range;
pub(crate) mod status;

pub(crate) use client::Clients;
pub(crate) use range::{RequestRange, check_partial, check_whole, content_range};
pub(crate) use status::{MAX_REDIRECTS, is_busy};

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
    /// `last_modified` when it is a strong validator: at least one second
    /// older than the answer's `Date` (RFC 9110, section 8.8.2.2). A server
    /// that stamps each answer with the time it sends it gives a date that
    /// says nothing about the file.
    pub strong_modified: Option<SystemTime>,
}

impl Validators {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        let text = |name| {
            headers
                .get(name)
                .and_then(|v: &HeaderValue| v.to_str().ok())
                .map(str::to_string)
        };
        let time = |name| httpdate::parse_http_date(text(name)?.trim()).ok();
        let strong_modified = match (time(LAST_MODIFIED), time(DATE)) {
            (Some(modified), Some(sent)) if modified + Duration::from_secs(1) <= sent => {
                Some(modified)
            }
            _ => None,
        };
        Self {
            etag: text(ETAG),
            last_modified: text(LAST_MODIFIED),
            strong_modified,
        }
    }

    /// Whether an answer carrying `answer` is about another version of the
    /// file than the one these validators came from. When both carry an
    /// `ETag`, it alone decides (RFC 9110, section 13.2.2 gives it precedence);
    /// otherwise a strong `Last-Modified` on both that differs does.
    ///
    /// This only refuses a contradiction; it does not ask for proof that the
    /// answers match. RFC 9110 (section 15.3.7.3) joins partial answers only
    /// when they share a strong validator, compared strongly; the engine
    /// departs from that on purpose, because many servers send only weak tags,
    /// or none on some answers, and refusing them would fail downloads that are
    /// sound. So entity tags are compared weakly (`W/"x"` matches `"x"`), two
    /// answers with the same weak tag count as not contradicted rather than
    /// proven the same, and a validator only one answer carries proves nothing.
    pub(crate) fn contradicts(&self, answer: &Validators) -> bool {
        if let (Some(known), Some(now)) = (&self.etag, &answer.etag) {
            return opaque_tag(known) != opaque_tag(now);
        }
        matches!(
            (self.strong_modified, answer.strong_modified),
            (Some(known), Some(now)) if known != now
        )
    }
}

/// An entity tag without its weakness mark: `W/"x"` and `"x"` are both `"x"`.
fn opaque_tag(tag: &str) -> &str {
    let tag = tag.trim();
    tag.strip_prefix("W/").unwrap_or(tag)
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
        Self::from_status(response.status(), response.headers(), response.url())
    }

    /// An answer with `status` and `headers` from `url` that is not the one
    /// wanted, over any version of HTTP: a busy answer keeps the wait its
    /// `Retry-After` asks for.
    pub(crate) fn from_status(status: StatusCode, headers: &HeaderMap, url: &Url) -> Self {
        let retry_after = is_busy(status)
            .then(|| headers.get(RETRY_AFTER))
            .flatten()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, SystemTime::now()));
        Self {
            error: Error::HttpStatus {
                status: status.as_u16(),
                url: url.to_string(),
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
    crate::url::without_credentials(url)
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

/// Sends `GET url`, following redirects. Each hop carries only the caller
/// headers its origin may see (see [`RequestHeaders`]), refuses to go from
/// `https` to `http` unless allowed, gives up waiting for an answer after the
/// stall timeout, and returns [`Error::Cancelled`] as soon as the download is
/// cancelled.
pub(crate) async fn send(
    client: &Client,
    url: &Url,
    session: &Session,
    range: Option<RequestRange>,
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
            request = request.header(RANGE, range.to_string());
        }
        let response = tokio::select! {
            biased;
            () = session.cancel.cancelled() => return Err(Error::Cancelled.into()),
            answer = tokio::time::timeout(session.answer_timeout, request.send()) => match answer {
                Ok(response) => response?,
                Err(_) => return Err(Error::Stalled {
                    after: session.answer_timeout,
                }.into()),
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
    /// A limited request was answered with part of the file and no size, so
    /// the body is not the whole file.
    limited_without_size: bool,
}

/// Sends `GET` with `Range: bytes=0-`. A `206` proves ranges work and gives the
/// size, and its body is already the start of the file, so no extra round trip
/// is spent on a separate `HEAD`.
///
/// With a `limit` ([`DownloadOptions::max_request_bytes`]) it asks for
/// `bytes=0-(limit-1)` instead, and the body is only the start of the file.
/// A server that answers that with a range but no size is asked again for
/// `bytes=0-`, as without a limit: with no size, the download cannot know
/// where the file ends except by reading one answer to its end.
pub(crate) async fn probe(
    client: &Client,
    url: &Url,
    session: &Session,
    limit: Option<u64>,
) -> Result<Probe, Failure> {
    if let Some(limit) = limit.filter(|&n| n > 0) {
        let range = RequestRange::new(0, limit).ok_or_else(|| empty_range(0, limit))?;
        let probe = probe_range(client, url, session, range).await?;
        if probe.limited_without_size {
            debug!("the server sent part of the file without its size; asking for all of it");
        } else {
            return Ok(probe);
        }
    }
    probe_range(client, url, session, RequestRange::from(0)).await
}

/// [`probe`] asking for `range`, which starts at byte 0.
async fn probe_range(
    client: &Client,
    url: &Url,
    session: &Session,
    range: RequestRange,
) -> Result<Probe, Failure> {
    let response = send(client, url, session, Some(range)).await?;
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

    let mut limited_without_size = false;
    let (ranged, total) = match status {
        StatusCode::PARTIAL_CONTENT => {
            if let Some(total) =
                check_partial(response_headers, 0, None, &Validators::default())?.total
            {
                (true, Some(total))
            } else {
                // Only the whole file if the request asked for all of it.
                limited_without_size = !range.is_whole();
                (false, None)
            }
        }
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
        limited_without_size,
    })
}

/// Requests `[from, to)` of the file `request` describes. Without range
/// support the whole body is requested and the first `from` bytes must be
/// skipped; the returned number says how many. Either way the answer must be
/// of the same file the first answer was about.
pub(crate) async fn fetch(
    client: &Client,
    request: &RequestContext,
    session: &Session,
    from: u64,
    to: u64,
) -> Result<(Response, u64), Failure> {
    if !request.ranged {
        let response = send(client, &request.url, session, None).await?;
        check_status(&response, StatusCode::OK)?;
        check_whole(
            response.headers(),
            response.content_length(),
            request.total,
            &request.validators,
        )?;
        return Ok((response, from));
    }

    let range = RequestRange::new(from, to).ok_or_else(|| empty_range(from, to))?;
    let response = send(client, &request.url, session, Some(range)).await?;
    if response.status() == StatusCode::OK {
        return Err(Error::RangeNotHonored {
            expected: from,
            got: None,
        }
        .into());
    }
    check_status(&response, StatusCode::PARTIAL_CONTENT)?;
    check_partial(response.headers(), from, request.total, &request.validators)?;
    Ok((response, 0))
}

/// The error for a request asked to fetch no bytes, which its caller rules out.
pub(crate) fn empty_range(from: u64, to: u64) -> Error {
    Error::Internal(format!("a request for no bytes ({from} to {to})"))
}

fn check_status(response: &Response, want: StatusCode) -> Result<(), Failure> {
    if response.status() == want {
        Ok(())
    } else {
        Err(Failure::status(response))
    }
}

#[cfg(test)]
mod tests;
