//! HTTP/3 over QUIC, for downloads with [`DownloadOptions::http3`] on.
//!
//! The first request always goes over HTTP/2 or HTTP/1.1. When its answer
//! advertises HTTP/3 on the same host (`Alt-Svc: h3=":443"`), each worker opens
//! a QUIC connection of its own (its own UDP socket and congestion control, as
//! each worker has its own TCP connection otherwise) and sends its range
//! requests there. Any QUIC failure marks HTTP/3 as broken for the rest of the
//! download, and every worker carries on over its TCP client from where it
//! was. Only a real answer from the server (`429` or `503`, which say "busy")
//! is treated like the same answer over TCP.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::{Buf, Bytes};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT_ENCODING, HeaderMap, HeaderValue, RANGE, USER_AGENT};
use tokio::task::JoinHandle;
use tracing::{debug, info};
use url::Url;

use crate::error::{Chain, Error, Result};
use crate::http::{self, Failure, RequestRange, Session};
use crate::options::DownloadOptions;
use crate::proxy::{self, Scheme};
use crate::request::RequestContext;

/// Longest wait for a QUIC handshake. A network that blocks UDP shows up as a
/// handshake that never completes; this bounds how long a worker spends
/// finding out before it falls back to TCP.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// One MiB as a `u32`, the type quinn's window settings take.
const MIB_U32: u32 = 1024 * 1024;

/// Entries in the HTTP/3 TLS session cache, shared by the QUIC connections of
/// one download: the same few hosts, with room to spare.
const SESSION_CACHE_ENTRIES: usize = 64;

/// Flow control windows. The defaults suit a 100 Mbit/s link with 100 ms of
/// latency; downloads can be much faster than that.
const STREAM_WINDOW: u32 = 8 * MIB_U32;
const CONNECTION_WINDOW: u32 = 16 * MIB_U32;

/// The HTTP/3 endpoint the server advertised, shared by every worker of one
/// download.
pub(crate) struct Origin {
    /// The host name, for TLS and the address lookup.
    host: String,
    /// The address the first (TCP) request reached, if known: QUIC goes to
    /// the same machine instead of whichever address a new lookup gives first.
    ip: Option<IpAddr>,
    port: u16,
    config: quinn::ClientConfig,
    handshake_timeout: Duration,
    /// [`DownloadOptions::user_agent`], which the TCP clients send on their own.
    user_agent: String,
    /// Set by the first QUIC failure; from then on every worker uses TCP.
    broken: AtomicBool,
}

impl Origin {
    /// The HTTP/3 endpoint the first answer advertised, if any. Only an
    /// endpoint on the same host is used: the credentials of the download
    /// belong to that host.
    ///
    /// `peer` is the address the first request reached. Nothing is used when
    /// a proxy is set for the host in the environment `env` reads: QUIC would
    /// go around it.
    pub fn advertised(
        url: &Url,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
        tls: &rustls::ClientConfig,
        opts: &DownloadOptions,
        env: proxy::Env<'_>,
    ) -> Option<Arc<Self>> {
        if url.scheme() != "https" {
            return None;
        }
        let host = url.host_str()?;
        if let Some(variable) = proxy::configured(Scheme::Https, host, env) {
            debug!(variable, "a proxy is set, so HTTP/3 is not used");
            return None;
        }
        let port = headers
            .get_all("alt-svc")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|value| h3_port(value, host))?;
        let mut tls = tls.clone();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        // A cache of its own: tickets from TCP connections do not work for QUIC.
        tls.resumption = rustls::client::Resumption::in_memory_sessions(SESSION_CACHE_ENTRIES);
        let crypto = match quinn::crypto::rustls::QuicClientConfig::try_from(tls) {
            Ok(crypto) => crypto,
            Err(e) => {
                debug!(error = %Chain(&e), "HTTP/3 is not possible with this TLS setup");
                return None;
            }
        };
        let mut transport = quinn::TransportConfig::default();
        transport
            .stream_receive_window(STREAM_WINDOW.into())
            .receive_window(CONNECTION_WINDOW.into());
        let mut config = quinn::ClientConfig::new(Arc::new(crypto));
        config.transport_config(Arc::new(transport));
        // `host_str` puts IPv6 addresses in brackets; TLS and lookups want them bare.
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        info!(%host, port, "the server offers HTTP/3; using it");
        Some(Arc::new(Self {
            host,
            ip: peer.map(|p| p.ip()),
            port,
            config,
            handshake_timeout: opts.connect_timeout.min(HANDSHAKE_TIMEOUT),
            user_agent: opts.user_agent.clone(),
            broken: AtomicBool::new(false),
        }))
    }

    fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }

    fn mark_broken(&self, why: &dyn std::fmt::Display) {
        if !self.broken.swap(true, Ordering::Relaxed) {
            info!(reason = %why, "HTTP/3 failed; continuing over HTTP/2 or HTTP/1.1");
        }
    }

    /// A new QUIC connection with HTTP/3 set up on it.
    async fn connect(&self) -> Result<Connection> {
        let addr = if let Some(ip) = self.ip {
            SocketAddr::new(ip, self.port)
        } else {
            let what = format!("could not find {}", self.host);
            tokio::net::lookup_host((self.host.as_str(), self.port))
                .await
                .map_err(|e| Error::socket(what.clone(), e, true))?
                .next()
                .ok_or_else(|| Error::socket(what, io::ErrorKind::NotFound.into(), true))?
        };
        let unspecified: IpAddr = if addr.is_ipv6() {
            Ipv6Addr::UNSPECIFIED.into()
        } else {
            Ipv4Addr::UNSPECIFIED.into()
        };
        let endpoint = quinn::Endpoint::client(SocketAddr::new(unspecified, 0))
            .map_err(|e| Error::socket("could not open a UDP socket for QUIC", e, true))?;
        let connecting = endpoint
            .connect_with(self.config.clone(), addr, &self.host)
            .map_err(|e| Error::transport("could not start a QUIC connection", e, true))?;
        let quic = tokio::time::timeout(self.handshake_timeout, connecting)
            .await
            .map_err(|_| {
                Error::socket(
                    "the QUIC handshake timed out",
                    io::ErrorKind::TimedOut.into(),
                    true,
                )
            })?
            .map_err(|e| Error::transport("the QUIC handshake failed", e, true))?;
        let (mut driver, send) = h3::client::new(h3_quinn::Connection::new(quic))
            .await
            .map_err(|e| Error::transport("setting up HTTP/3 failed", e, true))?;
        let driver = tokio::spawn(async move {
            // A request on a broken connection fails with its own error; this
            // says why the connection ended.
            let closed = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
            if !closed.is_h3_no_error() {
                debug!(error = %Chain(&closed), "an HTTP/3 connection closed");
            }
        });
        Ok(Connection {
            send,
            driver,
            _endpoint: endpoint,
        })
    }
}

/// The port of an `h3` alternative on `host` in an `Alt-Svc` value such as
/// `h3=":443"; ma=86400, h3-29=":443"`. Drafts (`h3-29`) are not used.
pub(crate) fn h3_port(value: &str, host: &str) -> Option<u16> {
    value.split(',').find_map(|entry| {
        let (protocol, rest) = entry.trim().split_once('=')?;
        if protocol.trim() != "h3" {
            return None;
        }
        let authority = rest.split(';').next()?.trim().trim_matches('"');
        let (alt_host, port) = authority.rsplit_once(':')?;
        if !alt_host.is_empty() && !alt_host.eq_ignore_ascii_case(host) {
            return None;
        }
        port.parse().ok().filter(|&p| p != 0)
    })
}

/// One worker's QUIC connection.
pub(crate) struct Connection {
    send: h3::client::SendRequest<h3_quinn::OpenStreams, Bytes>,
    driver: JoinHandle<()>,
    _endpoint: quinn::Endpoint,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.driver.abort();
    }
}

/// Requests `[from, to)` over HTTP/3. `Ok(None)` means HTTP/3 cannot be used
/// (now or anymore) and the caller should use TCP instead.
pub(crate) async fn open(
    slot: &mut Option<Connection>,
    origin: &Arc<Origin>,
    context: &RequestContext,
    session: &Session,
    from: u64,
    to: u64,
) -> Result<Option<Body>, Failure> {
    if origin.is_broken() {
        *slot = None;
        return Ok(None);
    }
    let attempt = request(slot, origin, context, session, from, to);
    let outcome = tokio::select! {
        biased;
        () = context.cancel.cancelled() => return Err(Error::Cancelled.into()),
        outcome = attempt => outcome,
    };
    match outcome {
        Ok(stream) => Ok(Some(Body {
            stream,
            chunk: Bytes::new(),
            origin: origin.clone(),
        })),
        Err(H3Failure::Answer(failure)) => Err(failure),
        Err(H3Failure::Unusable(why)) => {
            origin.mark_broken(&why);
            *slot = None;
            Ok(None)
        }
    }
}

/// Why a request over HTTP/3 did not start a body.
enum H3Failure {
    /// The server answered "busy", or with another version of the file:
    /// handled like the same answer over TCP.
    Answer(Failure),
    /// HTTP/3 does not work here.
    Unusable(Error),
}

type Stream = h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

async fn request(
    slot: &mut Option<Connection>,
    origin: &Origin,
    context: &RequestContext,
    session: &Session,
    from: u64,
    to: u64,
) -> std::result::Result<Stream, H3Failure> {
    let connection = match slot {
        Some(connection) => connection,
        empty => empty.insert(origin.connect().await.map_err(H3Failure::Unusable)?),
    };
    let stall = context.stall_timeout;
    let request = build_request(
        context,
        session,
        &origin.user_agent,
        RequestRange::new(from, to)
            .ok_or_else(|| H3Failure::Unusable(http::empty_range(from, to)))?,
    )?;
    let exchange = async {
        let mut stream = connection
            .send
            .send_request(request)
            .await
            .map_err(|e| unusable("sending the request over HTTP/3 failed", e))?;
        stream
            .finish()
            .await
            .map_err(|e| unusable("sending the request over HTTP/3 failed", e))?;
        let response = stream
            .recv_response()
            .await
            .map_err(|e| unusable("no answer over HTTP/3", e))?;
        Ok((stream, response))
    };
    let (stream, response) = tokio::time::timeout(stall, exchange)
        .await
        .map_err(|_| H3Failure::Unusable(Error::Stalled { after: stall }))??;
    check_response(&response, context, from)?;
    Ok(stream)
}

/// HTTP/3 is not usable because `error` happened while doing `what`.
fn unusable(what: &str, error: impl std::error::Error + Send + Sync + 'static) -> H3Failure {
    H3Failure::Unusable(Error::transport(what, error, false))
}

/// The request for `range` with the headers the session sends to this URL.
fn build_request(
    context: &RequestContext,
    session: &Session,
    user_agent: &str,
    range: RequestRange,
) -> std::result::Result<::http::Request<()>, H3Failure> {
    let range = HeaderValue::try_from(range.to_string())
        .map_err(|e| unusable("the range cannot be sent over HTTP/3", e))?;
    let mut request = ::http::Request::get(context.url.as_str());
    // Without headers the request already holds the error `body` reports.
    if let Some(headers) = request.headers_mut() {
        headers.extend(session.headers.for_url(&context.url).clone());
        if !headers.contains_key(USER_AGENT)
            && let Ok(agent) = HeaderValue::from_str(user_agent)
        {
            headers.insert(USER_AGENT, agent);
        }
        // Compressed transfer encodings break byte offsets.
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        headers.insert(RANGE, range);
    }
    request
        .body(())
        .map_err(|e| unusable("the request cannot be sent over HTTP/3", e))
}

/// Whether `response` is the range asked for, starting at `from`, of the same
/// file: a busy answer and a changed file are handled like the same answer over
/// TCP, any other answer means HTTP/3 is not usable here.
fn check_response(
    response: &::http::Response<()>,
    context: &RequestContext,
    from: u64,
) -> std::result::Result<(), H3Failure> {
    let status = response.status();
    if http::is_busy(status) {
        return Err(H3Failure::Answer(Failure::from_status(
            status,
            response.headers(),
            &context.url,
        )));
    }
    if status != StatusCode::PARTIAL_CONTENT {
        return Err(H3Failure::Unusable(Error::HttpStatus {
            status: status.as_u16(),
            url: context.url.to_string(),
        }));
    }
    match http::check_partial(response.headers(), from, context.total) {
        Ok(_) => Ok(()),
        Err(changed @ Error::RemoteChanged) => Err(H3Failure::Answer(changed.into())),
        Err(other) => Err(H3Failure::Unusable(other)),
    }
}

/// A response body arriving over HTTP/3.
pub(crate) struct Body {
    stream: Stream,
    chunk: Bytes,
    origin: Arc<Origin>,
}

impl Body {
    pub async fn next(&mut self) -> Result<Option<&[u8]>> {
        match self.stream.recv_data().await {
            Ok(Some(mut data)) => {
                // Zero-copy for the `Bytes` QUIC hands over.
                self.chunk = data.copy_to_bytes(data.remaining());
                Ok(Some(&self.chunk[..]))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::transport("the HTTP/3 stream failed", e, false)),
        }
    }

    /// Gives up HTTP/3 for the rest of the download.
    pub fn abandon(&self, why: &dyn std::fmt::Display) {
        self.origin.mark_broken(why);
    }
}

#[cfg(test)]
mod tests;
