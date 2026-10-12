//! What differs between protocols: the first request, opening a range on a
//! connection, and reading a body. Everything else (segments, resume files,
//! workers, retries) is shared by HTTP, HTTP/3 and FTP.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::{Client, Response};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::download::DownloadOptions;
use crate::error::{Error, Result};
use crate::ftp;
use crate::http::{self, Clients, Failure, Session, Validators};
#[cfg(feature = "http3")]
use crate::http3;
use crate::net_guard::Guard;
use crate::proxy::{self, Scheme};
use crate::retry::{Attempts, Next, Refusals};

/// What every request of one download needs, and all the protocol layers get
/// of it.
pub(crate) struct RequestContext {
    /// Where requests go: the URL after redirects, without credentials.
    pub url: Url,
    /// Whether the server accepts byte ranges.
    pub ranged: bool,
    /// The size of the file, if known.
    pub total: Option<u64>,
    /// Cancelled when the download pauses.
    pub cancel: CancellationToken,
    /// [`DownloadOptions::stall_timeout`].
    pub stall_timeout: Duration,
}

/// Where the file comes from; shared by every connection of one download.
#[expect(
    clippy::large_enum_variant,
    reason = "one value per download, shared by its connections, so the size of the larger variant does not matter"
)]
pub(crate) enum Remote {
    Http {
        /// Which headers go where, redirect rules and timeouts.
        session: Session,
        /// Set when HTTP/3 is switched on and the server advertised it.
        #[cfg(feature = "http3")]
        h3: Option<Arc<http3::Origin>>,
    },
    Ftp(Arc<ftp::Server>),
}

impl Remote {
    /// A new connection for one more worker.
    pub fn connection(&self, clients: &Clients) -> Result<Connection> {
        Ok(match self {
            Remote::Http { .. } => Connection::Http {
                client: clients.build()?,
                #[cfg(feature = "http3")]
                h3: None,
            },
            Remote::Ftp(server) => Connection::Ftp(Box::new(ftp::Connection::new(server.clone()))),
        })
    }
}

/// One worker's connection to the server. HTTP keeps a client of its own (and
/// so a TCP connection of its own); FTP keeps a logged-in control connection
/// between ranges.
pub(crate) enum Connection {
    Http {
        client: Client,
        #[cfg(feature = "http3")]
        h3: Option<http3::Connection>,
    },
    Ftp(Box<ftp::Connection>),
}

impl Connection {
    /// Starts receiving `[from, to)`. Returns the body and how many bytes at
    /// its start belong before `from` and must be skipped (a server without
    /// byte ranges always sends from the start).
    ///
    /// For HTTP, `to` is the end of this one request, which may stop short of
    /// the segment's end (see
    /// [`Shared::request_end`](crate::download::Shared::request_end)); FTP
    /// sends from `from` to the end of the file whatever `to` is.
    pub async fn open(
        &mut self,
        remote: &Remote,
        request: &RequestContext,
        from: u64,
        to: u64,
    ) -> Result<(Body, u64), Failure> {
        match (self, remote) {
            (Connection::Ftp(connection), Remote::Ftp(_)) => {
                let transfer = connection.open(request, from).await?;
                let skip = if request.ranged { 0 } else { from };
                Ok((Body::Ftp(transfer), skip))
            }
            #[cfg(feature = "http3")]
            (
                Connection::Http { client, h3 },
                Remote::Http {
                    session,
                    h3: origin,
                },
            ) => {
                if let Some(origin) = origin
                    && request.ranged
                    && let Some(body) = http3::open(h3, origin, request, session, from, to).await?
                {
                    return Ok((Body::Http3(Box::new(body)), 0));
                }
                let (response, skip) = http::fetch(
                    client,
                    &request.url,
                    session,
                    from,
                    to,
                    request.ranged,
                    request.total,
                )
                .await?;
                Ok((Body::http(response), skip))
            }
            #[cfg(not(feature = "http3"))]
            (Connection::Http { client }, Remote::Http { session }) => {
                let (response, skip) = http::fetch(
                    client,
                    &request.url,
                    session,
                    from,
                    to,
                    request.ranged,
                    request.total,
                )
                .await?;
                Ok((Body::http(response), skip))
            }
            _ => {
                Err(Error::Internal("a connection was paired with another protocol".into()).into())
            }
        }
    }

    /// Ends a transfer, whether it finished or not. An FTP control connection
    /// is made ready for the next range when it can be; anything else is
    /// simply closed.
    pub async fn finish(&mut self, body: Body, cancel: &CancellationToken) {
        if let (Connection::Ftp(connection), Body::Ftp(transfer)) = (self, body) {
            connection.finish(transfer, cancel).await;
        }
    }
}

/// A response body being received.
pub(crate) enum Body {
    Http {
        response: Response,
        /// The chunk last handed out; kept so [`Body::next`] can lend it.
        chunk: Bytes,
    },
    Ftp(ftp::Transfer),
    #[cfg(feature = "http3")]
    Http3(Box<http3::Body>),
}

impl Body {
    pub fn http(response: Response) -> Self {
        Body::Http {
            response,
            chunk: Bytes::new(),
        }
    }

    /// For a body arriving over HTTP/3: gives up HTTP/3 for the rest of the
    /// download because of `why`, and returns true, so the caller carries on
    /// over TCP. False for any other body.
    #[cfg(feature = "http3")]
    pub fn abandon_http3(&self, why: &dyn std::fmt::Display) -> bool {
        match self {
            Body::Http3(body) => {
                body.abandon(why);
                true
            }
            Body::Http { .. } | Body::Ftp(_) => false,
        }
    }

    /// False: without the `http3` feature no body arrives over HTTP/3.
    #[cfg(not(feature = "http3"))]
    pub fn abandon_http3(&self, _why: &dyn std::fmt::Display) -> bool {
        match self {
            Body::Http { .. } | Body::Ftp(_) => false,
        }
    }

    /// The next piece of the body, or `None` once it ended normally.
    pub async fn next(&mut self) -> Result<Option<&[u8]>> {
        match self {
            Body::Http { response, chunk } => {
                match response.chunk().await.map_err(Error::network)? {
                    Some(bytes) => {
                        *chunk = bytes;
                        Ok(Some(&chunk[..]))
                    }
                    None => Ok(None),
                }
            }
            Body::Ftp(transfer) => transfer.next().await,
            #[cfg(feature = "http3")]
            Body::Http3(body) => body.next().await,
        }
    }
}

/// What the first request found out, plus everything the download needs to
/// continue from there.
pub(crate) struct Opened {
    /// Where later requests go: the URL after redirects, without credentials.
    pub final_url: Url,
    pub ranged: bool,
    pub total: Option<u64>,
    pub validators: Validators,
    /// The name the server suggested (`Content-Disposition`), if any.
    pub file_name: Option<String>,
    pub content_type: Option<String>,
    /// The first response, already streaming from byte 0.
    pub body: Body,
    /// The connection `body` arrived on; it becomes the first worker's.
    pub connection: Connection,
    pub remote: Remote,
}

/// Sends the first request for `url`, retrying the way every later request is
/// retried, and picks the protocol from the URL's scheme.
pub(crate) async fn open(
    url: &Url,
    opts: &DownloadOptions,
    clients: &Clients,
    cancel: &CancellationToken,
    guard: Option<Guard>,
) -> Result<Opened> {
    match url.scheme() {
        "http" | "https" => open_http(url, opts, clients, cancel, guard).await,
        "ftp" | "ftps" | "ftpes" => {
            // Going around a proxy the user set would be a surprise, and
            // possibly a leak: refuse instead, until FTP proxies are supported.
            let host = url.host_str().unwrap_or_default();
            if let Some(variable) = proxy::configured(Scheme::Ftp, host, clients.environment()) {
                return Err(Error::FtpProxyUnsupported {
                    variable: variable.to_string(),
                });
            }
            let target = ftp::Target::new(url, opts, clients.tls())?;
            let probe = with_retries(opts, cancel, || ftp::probe(&target, cancel)).await?;
            Ok(Opened {
                final_url: target.display_url().clone(),
                ranged: probe.ranged,
                total: probe.total,
                validators: probe.validators,
                file_name: None,
                content_type: None,
                body: Body::Ftp(probe.transfer),
                connection: Connection::Ftp(Box::new(probe.connection)),
                remote: Remote::Ftp(probe.server),
            })
        }
        other => Err(Error::InvalidUrl(format!(
            "{} uses \"{other}\", which Wirecatch cannot download from. Links must start with http, https, ftp, ftps or ftpes",
            crate::url::without_password(url)
        ))),
    }
}

async fn open_http(
    url: &Url,
    opts: &DownloadOptions,
    clients: &Clients,
    cancel: &CancellationToken,
    guard: Option<Guard>,
) -> Result<Opened> {
    let (mut session, url) = Session::new(url, opts, cancel.clone());
    session.guard = guard;
    let client = clients.build()?;
    let limit = opts.request_limit();
    let probe = with_retries(opts, cancel, || http::probe(&client, &url, &session, limit)).await?;
    #[cfg(feature = "http3")]
    let h3 = if opts.http3 {
        http3::Origin::advertised(
            &probe.final_url,
            probe.response.headers(),
            probe.response.remote_addr(),
            clients.tls(),
            opts,
            clients.environment(),
        )
    } else {
        None
    };
    Ok(Opened {
        final_url: probe.final_url,
        ranged: probe.ranged,
        total: probe.total,
        validators: probe.validators,
        file_name: probe.file_name,
        content_type: probe.content_type,
        body: Body::http(probe.response),
        connection: Connection::Http {
            client,
            #[cfg(feature = "http3")]
            h3: None,
        },
        remote: Remote::Http {
            session,
            #[cfg(feature = "http3")]
            h3,
        },
    })
}

/// The first request gets the same retry treatment as every later one: a server
/// can drop a connection before sending any headers.
///
/// A 429 or 503 with `Retry-After` waits as long as the server asked (up to
/// five minutes) instead of the usual backoff.
async fn with_retries<T, F, Fut>(
    opts: &DownloadOptions,
    cancel: &CancellationToken,
    mut attempt: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Failure>>,
{
    let mut attempts = Attempts::new(Refusals::AsFailures, opts.max_retries);
    loop {
        let failure = match attempt().await {
            Ok(value) => return Ok(value),
            Err(failure) => failure,
        };
        let delay = match attempts.after_failure(&failure.error, failure.retry_after, false) {
            Next::GiveUp => return Err(failure.error),
            Next::Retry(delay) | Next::StepBackOrRetry(delay) => delay,
        };
        tracing::warn!(error = %failure.error, retry_in = ?delay, "first request failed, retrying");
        tokio::select! {
            () = cancel.cancelled() => return Err(Error::Cancelled),
            () = tokio::time::sleep(delay) => {}
        }
    }
}
