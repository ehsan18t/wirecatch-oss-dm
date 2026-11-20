//! The guard's way through the browser (D282): a tunnel yt-dlp opens to a host of the page's site is answered here, over TLS with a certificate of a per-run authority, and each request in it is made by the browser. yt-dlp trusts the authority through its own environment only; nothing goes into the computer's trust store.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine as _;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use wirecatch_engine::Url;
use wirecatch_product::PRODUCT_NAME;
use wirecatch_protocol::hosts::host_on_site;
use wirecatch_protocol::page_read::MAX_PAGE_READ_BODY;

use super::browser::{BrowserAnswer, BrowserRequest, BrowserRoute};
use super::guard::{BAD_REQUEST, TUNNEL_OPEN, UNREACHABLE, read_head};

/// How many base64 characters a PEM line holds (RFC 7468).
const PEM_LINE: usize = 64;

/// Headers of the browser's answer that described it on the browser's own connection: its body comes decoded and whole.
const NOT_PASSED_ON: [&str; 5] = [
    "connection",
    "keep-alive",
    "content-length",
    "content-encoding",
    "transfer-encoding",
];

/// The run's certificate authority, which lives only in memory, and the bundle yt-dlp trusts.
pub(super) struct Authority {
    params: CertificateParams,
    key: KeyPair,
    der: CertificateDer<'static>,
    /// The computer's own roots and this authority, in PEM: for `SSL_CERT_FILE`.
    bundle: String,
    /// A server configuration per host, made at the first tunnel to it.
    configs: Mutex<HashMap<String, Arc<ServerConfig>>>,
}

impl Authority {
    /// A new authority; it reads the computer's roots, so it runs off the async threads.
    pub(super) fn new() -> Result<Self, rcgen::Error> {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params
            .distinguished_name
            .push(DnType::CommonName, format!("{PRODUCT_NAME} page reads"));
        // It signs host certificates only, never another authority.
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let key = KeyPair::generate()?;
        let der = params.self_signed(&key)?.der().clone();
        let mut bundle = system_roots().to_owned();
        push_pem(&mut bundle, &der);
        Ok(Self {
            params,
            key,
            der,
            bundle,
            configs: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn bundle(&self) -> &str {
        &self.bundle
    }

    fn config_for(&self, host: &str) -> io::Result<Arc<ServerConfig>> {
        let mut configs = self
            .configs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(config) = configs.get(host) {
            return Ok(config.clone());
        }
        let config = Arc::new(self.server_config(host).map_err(io::Error::other)?);
        configs.insert(host.to_owned(), config.clone());
        Ok(config)
    }

    fn server_config(
        &self,
        host: &str,
    ) -> Result<ServerConfig, Box<dyn std::error::Error + Send + Sync>> {
        let params = CertificateParams::new(vec![host.to_owned()])?;
        let key = KeyPair::generate()?;
        let leaf = params.signed_by(&key, &Issuer::from_params(&self.params, &self.key))?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(rustls::ALL_VERSIONS)?
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone(), self.der.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )?;
        // yt-dlp's requests are read here as HTTP/1.1 only.
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(config)
    }
}

/// The computer's trusted roots in PEM, read once: yt-dlp's other hosts keep being checked against them.
fn system_roots() -> &'static str {
    static ROOTS: OnceLock<String> = OnceLock::new();
    ROOTS.get_or_init(|| {
        // Windows holds only the roots it has fetched so far, so the bundle yt-dlp would use goes in too.
        let found = rustls_native_certs::load_native_certs();
        if !found.errors.is_empty() {
            tracing::debug!(
                errors = found.errors.len(),
                "some of the computer's root certificates could not be read"
            );
        }
        let mut pem = String::new();
        for der in found
            .certs
            .iter()
            .chain(webpki_root_certs::TLS_SERVER_ROOT_CERTS)
        {
            push_pem(&mut pem, der);
        }
        pem
    })
}

fn push_pem(pem: &mut String, der: &[u8]) {
    pem.push_str("-----BEGIN CERTIFICATE-----\n");
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    for line in encoded.as_bytes().chunks(PEM_LINE) {
        pem.push_str(&String::from_utf8_lossy(line));
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
}

/// What one run's tunnels through the browser share.
pub(super) struct Through {
    pub(super) route: BrowserRoute,
    pub(super) authority: Authority,
    /// Set when the browser made no answer to a request: the run's later tunnels go directly, and a run that failed reads again directly.
    pub(super) failed: AtomicBool,
}

impl Through {
    pub(super) fn new(route: BrowserRoute, authority: Authority) -> Self {
        Self {
            route,
            authority,
            failed: AtomicBool::new(false),
        }
    }

    /// Whether a tunnel to `host` goes through the browser: HTTPS to a host of the page's site, until the browser once made no answer. yt-dlp retries a failed request (a file too large for a page read, say), which then goes directly.
    pub(super) fn takes(&self, host: &str, port: u16) -> bool {
        port == 443 && host_on_site(host, &self.route.site) && !self.failed.load(Ordering::Relaxed)
    }
}

/// Answers a tunnel yt-dlp opened to `host`: one request over TLS, made by the browser.
pub(super) async fn carry(
    mut client: TcpStream,
    rest: &[u8],
    host: &str,
    through: &Through,
) -> io::Result<()> {
    // yt-dlp sends nothing before the tunnel opens; bytes here could not be part of the TLS handshake.
    if !rest.is_empty() {
        return client.write_all(BAD_REQUEST).await;
    }
    let config = through.authority.config_for(host)?;
    client.write_all(TUNNEL_OPEN).await?;
    let mut tls = TlsAcceptor::from(config).accept(client).await?;
    let Some((head, body)) = read_head(&mut tls, Vec::new()).await else {
        return Ok(());
    };
    let Some((mut request, body_len)) = request_of(&head, host) else {
        return tls.write_all(BAD_REQUEST).await;
    };
    request.body = body;
    if body_len > MAX_PAGE_READ_BODY || request.body.len() > body_len {
        return tls.write_all(BAD_REQUEST).await;
    }
    let left = body_len - request.body.len();
    let mut more = vec![0_u8; left];
    tls.read_exact(&mut more).await?;
    request.body.extend(more);
    let head_only = request.method.eq_ignore_ascii_case("HEAD");
    match through.route.reader.read(request).await {
        Ok(answer) => write_answer(&mut tls, &answer, head_only).await?,
        Err(failed) => {
            through.failed.store(true, Ordering::Relaxed);
            tracing::debug!(error = %failed, "the browser made no answer to a page read");
            tls.write_all(UNREACHABLE).await?;
        }
    }
    tls.shutdown().await
}

/// The request in a head read inside a tunnel to `host`, and the length of its body. A body of unstated length is refused.
fn request_of(head: &[u8], host: &str) -> Option<(BrowserRequest, usize)> {
    let text = std::str::from_utf8(head).ok()?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?, first.next()?);
    if !target.starts_with('/') {
        return None;
    }
    let url = Url::parse(&format!("https://{host}{target}")).ok()?;
    let mut headers = Vec::new();
    let mut body_len = 0;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("transfer-encoding") || name.eq_ignore_ascii_case("expect") {
            return None;
        }
        if name.eq_ignore_ascii_case("content-length") {
            body_len = value.parse().ok()?;
        }
        headers.push((name.to_owned(), value.to_owned()));
    }
    Some((
        BrowserRequest {
            method: method.to_owned(),
            url,
            headers,
            body: Vec::new(),
        },
        body_len,
    ))
}

async fn write_answer<S: AsyncWrite + Unpin>(
    out: &mut S,
    answer: &BrowserAnswer,
    head_only: bool,
) -> io::Result<()> {
    let reason = http::StatusCode::from_u16(answer.status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .unwrap_or("Unknown");
    // A HEAD answer has no body: its length is the one the site said.
    let length = if head_only {
        answer
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<u64>().ok())
            .unwrap_or_default()
    } else {
        answer.body.len() as u64
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nConnection: close\r\nContent-Length: {length}\r\n",
        answer.status
    );
    for (name, value) in &answer.headers {
        let passed_on = !NOT_PASSED_ON.iter().any(|n| name.eq_ignore_ascii_case(n));
        if passed_on && !name.contains(['\r', '\n']) && !value.contains(['\r', '\n']) {
            for piece in [name.as_str(), ": ", value.as_str(), "\r\n"] {
                head.push_str(piece);
            }
        }
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes()).await?;
    if !head_only {
        out.write_all(&answer.body).await?;
    }
    Ok(())
}

/// Read by the tests with a client that trusts only the run's authority.
#[cfg(test)]
pub(super) fn authority_der(authority: &Authority) -> CertificateDer<'static> {
    authority.der.clone()
}
