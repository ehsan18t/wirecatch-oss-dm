//! HTTPS test servers for HTTP/3: a TCP server speaking HTTP/2 and HTTP/1.1
//! that can advertise HTTP/3 with `Alt-Svc`, an HTTP/3 server over QUIC, and
//! a UDP port that swallows every packet (QUIC blocked). All of them serve one
//! byte buffer with range support and count what they serve.

use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::header::{ALT_SVC, CONTENT_LENGTH, CONTENT_RANGE, HeaderMap, RANGE};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};

use crate::TestCertificate;
use crate::http::index;

/// How much the QUIC server may have in flight on one connection.
const QUIC_SEND_WINDOW: u64 = 32 * 1024 * 1024;
/// How much of a body the HTTP/3 server sends at once.
const H3_CHUNK: usize = 256 * 1024;

/// What a server has served.
#[derive(Default)]
pub struct Counts {
    /// Requests answered.
    pub requests: AtomicUsize,
    /// Requests that arrived over HTTP/2.
    pub http2: AtomicUsize,
    /// Body bytes sent.
    pub bytes: AtomicU64,
    /// The most bytes one request asked for; `u64::MAX` once a request asked
    /// for a range with no end, or for no range.
    pub largest_ask: AtomicU64,
}

/// How many bytes a request asks for: `u64::MAX` for a range with no end, or
/// for no range at all.
fn asked(headers: &HeaderMap) -> u64 {
    headers
        .get(RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(first, last)| Some((first.parse::<u64>().ok()?, last.parse::<u64>().ok()?)))
        .map_or(u64::MAX, |(start, last)| {
            last.saturating_sub(start).saturating_add(1)
        })
}

/// `bytes=a-b` or `bytes=a-` as a half-open range within `len`.
fn range(headers: &HeaderMap, len: u64) -> Option<(u64, u64)> {
    let spec = headers.get(RANGE)?.to_str().ok()?.strip_prefix("bytes=")?;
    let (first, last) = spec.split_once('-')?;
    let start: u64 = first.parse().ok()?;
    let end = if last.is_empty() {
        len
    } else {
        last.parse::<u64>().ok()?.saturating_add(1).min(len)
    };
    (start < end).then_some((start, end))
}

/// The status, headers and body slice for a request.
fn answer(data: &Bytes, headers: &HeaderMap) -> (StatusCode, HeaderMap, Bytes) {
    let len = data.len() as u64;
    let mut out = HeaderMap::new();
    let (status, start, end) = match range(headers, len) {
        Some((start, end)) => {
            let value = format!("bytes {start}-{}/{len}", end - 1);
            out.insert(CONTENT_RANGE, value.parse().unwrap());
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        None => (StatusCode::OK, 0, len),
    };
    out.insert(CONTENT_LENGTH, (end - start).into());
    (status, out, data.slice(index(start)..index(end)))
}

/// An HTTPS server on TCP (HTTP/2 or HTTP/1.1, as the client prefers) that
/// sends `alt_svc` as `Alt-Svc` when given. Returns its port.
pub async fn start_tcp(
    data: Bytes,
    cert: &TestCertificate,
    alt_svc: Option<String>,
) -> (u16, Arc<Counts>) {
    let acceptor =
        tokio_rustls::TlsAcceptor::from(Arc::new(cert.server_config(&[b"h2", b"http/1.1"])));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let counts = Arc::new(Counts::default());
    let served = counts.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, data, counts, alt_svc) = (
                acceptor.clone(),
                data.clone(),
                served.clone(),
                alt_svc.clone(),
            );
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let service = hyper::service::service_fn(move |request: Request<Incoming>| {
                    let (status, mut headers, body) = answer(&data, request.headers());
                    counts.requests.fetch_add(1, Ordering::SeqCst);
                    counts
                        .largest_ask
                        .fetch_max(asked(request.headers()), Ordering::SeqCst);
                    if request.version() == hyper::Version::HTTP_2 {
                        counts.http2.fetch_add(1, Ordering::SeqCst);
                    }
                    counts.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
                    if let Some(alt_svc) = &alt_svc {
                        headers.insert(ALT_SVC, alt_svc.parse().unwrap());
                    }
                    let mut response = Response::new(Full(Some(body)));
                    *response.status_mut() = status;
                    *response.headers_mut() = headers;
                    async move { Ok::<_, Infallible>(response) }
                });
                // The connection ends when the client closes it; how is not the test's concern.
                if let Err(error) =
                    hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(tls), service)
                        .await
                {
                    drop(error);
                }
            });
        }
    });
    (port, counts)
}

/// A body of one `Bytes`, without pulling in another crate for it.
struct Full(Option<Bytes>);

impl hyper::body::Body for Full {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Bytes>, Infallible>>> {
        std::task::Poll::Ready(self.0.take().map(|b| Ok(hyper::body::Frame::data(b))))
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::SizeHint::with_exact(self.0.as_ref().map_or(0, |b| b.len() as u64))
    }

    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}

/// An HTTP/3 server on a UDP port of its own. Returns the port. Needs a
/// Tokio runtime.
#[must_use]
pub fn start_h3(data: Bytes, cert: &TestCertificate) -> (u16, Arc<Counts>) {
    start_h3_breaking(data, cert, None, Duration::ZERO)
}

/// Like [`start_h3`], but every response stream is reset after
/// `break_after` bytes and a pause of `pause`, as a broken QUIC path would.
/// Needs a Tokio runtime.
#[must_use]
pub fn start_h3_breaking(
    data: Bytes,
    cert: &TestCertificate,
    break_after: Option<usize>,
    pause: Duration,
) -> (u16, Arc<Counts>) {
    let tls = cert.server_config(&[b"h3"]);
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap();
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.send_window(QUIC_SEND_WINDOW);
    config.transport_config(Arc::new(transport));
    let endpoint = quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let port = endpoint.local_addr().unwrap().port();
    let counts = Arc::new(Counts::default());
    let served = counts.clone();
    tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let (data, counts) = (data.clone(), served.clone());
            tokio::spawn(async move {
                let Ok(quic) = incoming.await else { return };
                let Ok(mut h3) =
                    h3::server::Connection::<_, Bytes>::new(h3_quinn::Connection::new(quic)).await
                else {
                    return;
                };
                while let Ok(Some(resolver)) = h3.accept().await {
                    let (data, counts) = (data.clone(), counts.clone());
                    tokio::spawn(async move {
                        if let Ok((request, stream)) = resolver.resolve_request().await {
                            respond(request, stream, &data, &counts, break_after, pause).await;
                        }
                    });
                }
            });
        }
    });
    (port, counts)
}

/// Answers one HTTP/3 request, breaking the stream off as configured.
async fn respond(
    request: http::Request<()>,
    mut stream: h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>,
    data: &Bytes,
    counts: &Counts,
    break_after: Option<usize>,
    pause: Duration,
) {
    let (status, headers, body) = answer(data, request.headers());
    counts.requests.fetch_add(1, Ordering::SeqCst);
    counts
        .largest_ask
        .fetch_max(asked(request.headers()), Ordering::SeqCst);
    let mut response = http::Response::new(());
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    if stream.send_response(response).await.is_err() {
        return;
    }
    if let Some(limit) = break_after {
        let cut = body.slice(..limit.min(body.len()));
        // The stream is reset next anyway; whether this part arrived is what the test checks.
        if let Err(error) = stream.send_data(cut).await {
            drop(error);
        }
        tokio::time::sleep(pause).await;
        stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
        return;
    }
    // The client stops reading once its range is split off.
    for chunk in body.chunks(H3_CHUNK) {
        let chunk = body.slice_ref(chunk);
        let n = chunk.len() as u64;
        if stream.send_data(chunk).await.is_err() {
            return;
        }
        counts.bytes.fetch_add(n, Ordering::Relaxed);
    }
    // A client that already has its range may close first; that is fine.
    if let Err(error) = stream.finish().await {
        drop(error);
    }
}

/// A UDP port that receives and drops everything, the way a firewall that
/// blocks QUIC looks from the client. It notes every address that sent to it:
/// each QUIC connection the engine tries comes from a UDP socket of its own,
/// so the count is the number of attempts. Keep it alive while it is used.
pub struct Blackhole {
    /// The port to advertise.
    pub port: u16,
    senders: Arc<Mutex<HashSet<SocketAddr>>>,
    reader: tokio::task::JoinHandle<()>,
}

impl Blackhole {
    /// Binds a port on 127.0.0.1 and starts swallowing what arrives.
    pub async fn start() -> Self {
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        let senders = Arc::new(Mutex::new(HashSet::new()));
        let reader = tokio::spawn({
            let senders = senders.clone();
            async move {
                let mut packet = [0u8; 2048];
                while let Ok((_, from)) = socket.recv_from(&mut packet).await {
                    senders.lock().unwrap().insert(from);
                }
            }
        });
        Self {
            port,
            senders,
            reader,
        }
    }

    /// How many QUIC connections were tried.
    #[must_use]
    pub fn attempts(&self) -> usize {
        self.senders.lock().unwrap().len()
    }
}

impl Drop for Blackhole {
    fn drop(&mut self) {
        self.reader.abort();
    }
}
