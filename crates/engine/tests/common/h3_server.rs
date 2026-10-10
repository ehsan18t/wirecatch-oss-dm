//! HTTPS test servers for HTTP/3: a TCP server speaking HTTP/2 and HTTP/1.1
//! that can advertise HTTP/3 with `Alt-Svc`, an HTTP/3 server over QUIC, and
//! a UDP port that swallows every packet (QUIC blocked). All of them serve one
//! byte buffer with range support and count what they serve.
//!
//! Needs the engine's `http3` feature (for quinn and h3). It does not depend
//! on the engine, so its unit tests and the benchmark example can both use it.

#![allow(dead_code)]

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::header::{ALT_SVC, CONTENT_LENGTH, CONTENT_RANGE, HeaderMap, RANGE};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// A self-signed certificate for 127.0.0.1 and localhost.
pub struct Cert {
    pub chain: Vec<CertificateDer<'static>>,
    key: PrivatePkcs8KeyDer<'static>,
    /// The certificate in PEM, for `SSL_CERT_FILE`.
    pub pem: String,
    pub roots: rustls::RootCertStore,
}

impl Cert {
    pub fn new() -> Self {
        let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into(), "localhost".into()])
            .unwrap();
        let der = CertificateDer::from(cert.cert.der().to_vec());
        let mut roots = rustls::RootCertStore::empty();
        roots.add(der.clone()).unwrap();
        Self {
            pem: pem(&der),
            chain: vec![der],
            key: PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()),
            roots,
        }
    }

    fn server_config(&self, alpn: &[&[u8]]) -> rustls::ServerConfig {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(rustls::ALL_VERSIONS)
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                self.chain.clone(),
                PrivateKeyDer::Pkcs8(self.key.clone_key()),
            )
            .unwrap();
        config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        config
    }
}

fn pem(der: &[u8]) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).unwrap());
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

#[derive(Default)]
pub struct Counts {
    pub requests: AtomicUsize,
    /// Requests that arrived over HTTP/2.
    pub http2: AtomicUsize,
    pub bytes: AtomicU64,
}

/// `bytes=a-b` or `bytes=a-` as a half-open range within `len`.
fn range(headers: &HeaderMap, len: u64) -> Option<(u64, u64)> {
    let spec = headers.get(RANGE)?.to_str().ok()?.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    let start: u64 = a.parse().ok()?;
    let end = if b.is_empty() {
        len
    } else {
        b.parse::<u64>().ok()?.saturating_add(1).min(len)
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
    (status, out, data.slice(start as usize..end as usize))
}

/// An HTTPS server on TCP (HTTP/2 or HTTP/1.1, as the client prefers) that
/// sends `alt_svc` as `Alt-Svc` when given. Returns its port.
pub async fn start_tcp(data: Bytes, cert: &Cert, alt_svc: Option<String>) -> (u16, Arc<Counts>) {
    let acceptor =
        tokio_rustls::TlsAcceptor::from(Arc::new(cert.server_config(&[b"h2", b"http/1.1"])));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let counts = Arc::new(Counts::default());
    let served = counts.clone();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
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
                    if request.version() == hyper::Version::HTTP_2 {
                        counts.http2.fetch_add(1, Ordering::SeqCst);
                    }
                    counts.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
                    if let Some(alt_svc) = &alt_svc {
                        headers.insert(ALT_SVC, alt_svc.parse().unwrap());
                    }
                    let mut response = Response::new(http_body_full(body));
                    *response.status_mut() = status;
                    *response.headers_mut() = headers;
                    async move { Ok::<_, Infallible>(response) }
                });
                let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                    .serve_connection(TokioIo::new(tls), service)
                    .await;
            });
        }
    });
    (port, counts)
}

/// A body of one `Bytes`, without pulling in another crate for it.
fn http_body_full(bytes: Bytes) -> Full {
    Full(Some(bytes))
}

pub struct Full(Option<Bytes>);

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

/// An HTTP/3 server on a UDP port of its own. Returns the port.
pub async fn start_h3(data: Bytes, cert: &Cert) -> (u16, Arc<Counts>) {
    start_h3_breaking(data, cert, None).await
}

/// Like [`start_h3`], but every response stream is reset after
/// `break_after` bytes, as a broken QUIC path would.
pub async fn start_h3_breaking(
    data: Bytes,
    cert: &Cert,
    break_after: Option<usize>,
) -> (u16, Arc<Counts>) {
    let tls = cert.server_config(&[b"h3"]);
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap();
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.send_window(32 * 1024 * 1024);
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
                        let Ok((request, mut stream)) = resolver.resolve_request().await else {
                            return;
                        };
                        let (status, headers, body) = answer(&data, request.headers());
                        counts.requests.fetch_add(1, Ordering::SeqCst);
                        let mut response = http::Response::new(());
                        *response.status_mut() = status;
                        *response.headers_mut() = headers;
                        if stream.send_response(response).await.is_err() {
                            return;
                        }
                        if let Some(limit) = break_after {
                            let cut = body.slice(..limit.min(body.len()));
                            let _ = stream.send_data(cut).await;
                            stream.stop_stream(h3::error::Code::H3_INTERNAL_ERROR);
                            return;
                        }
                        // The client stops reading once its range is split off.
                        for chunk in body.chunks(256 * 1024) {
                            let chunk = body.slice_ref(chunk);
                            let n = chunk.len() as u64;
                            if stream.send_data(chunk).await.is_err() {
                                return;
                            }
                            counts.bytes.fetch_add(n, Ordering::Relaxed);
                        }
                        let _ = stream.finish().await;
                    });
                }
            });
        }
    });
    (port, counts)
}

/// A UDP port that receives and drops everything, the way a firewall that
/// blocks QUIC looks from the client. Keep the socket alive while it is used.
pub fn blackhole() -> (std::net::UdpSocket, u16) {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
}
