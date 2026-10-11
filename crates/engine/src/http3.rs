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

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::{Buf, Bytes};
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_RANGE, HeaderMap, HeaderValue, RANGE, RETRY_AFTER, USER_AGENT,
};
use tokio::task::JoinHandle;
use tracing::{debug, info};
use url::Url;

use crate::download::{DownloadOptions, Shared};
use crate::error::{Error, Result};
use crate::http::{Failure, Session, parse_content_range, parse_retry_after};
use crate::transport::proxy_variable;

/// Longest wait for a QUIC handshake. A network that blocks UDP shows up as a
/// handshake that never completes; this bounds how long a worker spends
/// finding out before it falls back to TCP.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);

/// Flow control windows. The defaults suit a 100 Mbit/s link with 100 ms of
/// latency; downloads can be much faster than that.
const STREAM_WINDOW: u32 = 8 * 1024 * 1024;
const CONNECTION_WINDOW: u32 = 16 * 1024 * 1024;

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
    /// Set by the first QUIC failure; from then on every worker uses TCP.
    broken: AtomicBool,
}

impl Origin {
    /// The HTTP/3 endpoint the first answer advertised, if any. Only an
    /// endpoint on the same host is used: the credentials of the download
    /// belong to that host.
    ///
    /// `peer` is the address the first request reached. Nothing is used when
    /// a proxy is set in the environment: QUIC would go around it.
    pub fn advertised(
        url: &Url,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
        tls: &rustls::ClientConfig,
        opts: &DownloadOptions,
    ) -> Option<Arc<Self>> {
        Self::advertised_in(url, headers, peer, tls, opts, &|name| {
            std::env::var_os(name)
        })
    }

    /// [`Origin::advertised`], reading environment variables with `env`.
    fn advertised_in(
        url: &Url,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
        tls: &rustls::ClientConfig,
        opts: &DownloadOptions,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Option<Arc<Self>> {
        if url.scheme() != "https" {
            return None;
        }
        if let Some(variable) = proxy_variable(HTTPS_PROXY_VARIABLES, env) {
            debug!(variable, "a proxy is set, so HTTP/3 is not used");
            return None;
        }
        let host = url.host_str()?;
        let port = headers
            .get_all("alt-svc")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|value| h3_port(value, host))?;
        let mut tls = tls.clone();
        tls.alpn_protocols = vec![b"h3".to_vec()];
        // A cache of its own: tickets from TCP connections do not work for QUIC.
        tls.resumption = rustls::client::Resumption::in_memory_sessions(64);
        let crypto = match quinn::crypto::rustls::QuicClientConfig::try_from(tls) {
            Ok(crypto) => crypto,
            Err(e) => {
                debug!(error = %e, "HTTP/3 is not possible with this TLS setup");
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
    async fn connect(&self) -> std::result::Result<Connection, String> {
        let addr = match self.ip {
            Some(ip) => SocketAddr::new(ip, self.port),
            None => tokio::net::lookup_host((self.host.as_str(), self.port))
                .await
                .map_err(|e| format!("address lookup failed: {e}"))?
                .next()
                .ok_or("no address found")?,
        };
        let local: SocketAddr = if addr.is_ipv6() {
            "[::]:0".parse().map_err(|e| format!("{e}"))?
        } else {
            "0.0.0.0:0".parse().map_err(|e| format!("{e}"))?
        };
        let endpoint = quinn::Endpoint::client(local).map_err(|e| format!("no UDP socket: {e}"))?;
        let connecting = endpoint
            .connect_with(self.config.clone(), addr, &self.host)
            .map_err(|e| format!("{e}"))?;
        let quic = tokio::time::timeout(self.handshake_timeout, connecting)
            .await
            .map_err(|_| "the QUIC handshake timed out".to_string())?
            .map_err(|e| format!("the QUIC handshake failed: {e}"))?;
        let (mut driver, send) = h3::client::new(h3_quinn::Connection::new(quic))
            .await
            .map_err(|e| format!("HTTP/3 setup failed: {e}"))?;
        let driver = tokio::spawn(async move {
            let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
        });
        Ok(Connection {
            send,
            driver,
            _endpoint: endpoint,
        })
    }
}

/// The variables the HTTP client takes an `https` proxy from.
const HTTPS_PROXY_VARIABLES: &[&str] = &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

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
    shared: &Shared,
    session: &Session,
    from: u64,
    to: u64,
) -> Result<Option<Body>, Failure> {
    if origin.is_broken() {
        *slot = None;
        return Ok(None);
    }
    let attempt = request(slot, origin, shared, session, from, to);
    let outcome = tokio::select! {
        biased;
        _ = shared.cancel.cancelled() => return Err(Error::Cancelled.into()),
        outcome = attempt => outcome,
    };
    match outcome {
        Ok(stream) => Ok(Some(Body {
            stream,
            chunk: Bytes::new(),
            origin: origin.clone(),
        })),
        Err(Outcome::Answer(failure)) => Err(failure),
        Err(Outcome::Unusable(why)) => {
            origin.mark_broken(&why);
            *slot = None;
            Ok(None)
        }
    }
}

/// Why a request over HTTP/3 did not start a body.
enum Outcome {
    /// The server answered "busy": handled like the same answer over TCP.
    Answer(Failure),
    /// HTTP/3 does not work here.
    Unusable(String),
}

type Stream = h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

async fn request(
    slot: &mut Option<Connection>,
    origin: &Origin,
    shared: &Shared,
    session: &Session,
    from: u64,
    to: u64,
) -> std::result::Result<Stream, Outcome> {
    if slot.is_none() {
        *slot = Some(origin.connect().await.map_err(Outcome::Unusable)?);
    }
    let Some(connection) = slot.as_mut() else {
        return Err(Outcome::Unusable("no connection".into()));
    };

    let mut request = ::http::Request::get(shared.url.as_str());
    let headers = request
        .headers_mut()
        .ok_or_else(|| Outcome::Unusable("bad request".into()))?;
    headers.extend(session.headers.for_url(&shared.url).clone());
    if !headers.contains_key(USER_AGENT)
        && let Ok(agent) = HeaderValue::from_str(&shared.opts.user_agent)
    {
        headers.insert(USER_AGENT, agent);
    }
    // Compressed transfer encodings break byte offsets.
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    let range = if to == crate::segment::UNKNOWN_END {
        format!("bytes={from}-")
    } else {
        format!("bytes={from}-{}", to - 1)
    };
    let range = HeaderValue::from_str(&range).map_err(|e| Outcome::Unusable(e.to_string()))?;
    headers.insert(RANGE, range);
    let request = request
        .body(())
        .map_err(|e| Outcome::Unusable(e.to_string()))?;

    let stall = shared.opts.stall_timeout;
    let exchange = async {
        let mut stream = connection
            .send
            .send_request(request)
            .await
            .map_err(|e| Outcome::Unusable(format!("sending the request failed: {e}")))?;
        stream
            .finish()
            .await
            .map_err(|e| Outcome::Unusable(format!("sending the request failed: {e}")))?;
        let response = stream
            .recv_response()
            .await
            .map_err(|e| Outcome::Unusable(format!("no answer: {e}")))?;
        Ok((stream, response))
    };
    let (stream, response) = tokio::time::timeout(stall, exchange)
        .await
        .map_err(|_| Outcome::Unusable("no answer in time".into()))??;

    let status = response.status().as_u16();
    if status == 429 || status == 503 {
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, std::time::SystemTime::now()));
        return Err(Outcome::Answer(Failure {
            error: Error::HttpStatus {
                status,
                url: shared.url.to_string(),
            },
            retry_after,
        }));
    }
    if status != 206 {
        return Err(Outcome::Unusable(format!(
            "the server answered {status} over HTTP/3"
        )));
    }
    let range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_range);
    match range {
        Some(range) if range.start == from => {
            if let (Some(expected), Some(actual)) = (shared.total, range.total)
                && expected != actual
            {
                return Err(Outcome::Answer(Error::RemoteChanged.into()));
            }
            Ok(stream)
        }
        _ => Err(Outcome::Unusable(
            "the server sent the wrong range over HTTP/3".into(),
        )),
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
