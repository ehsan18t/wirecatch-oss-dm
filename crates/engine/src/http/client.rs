//! The HTTP clients of a download: one per connection, sharing one TLS setup.

use std::ffi::OsStr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use reqwest::header::{ACCEPT_ENCODING, HeaderMap, HeaderValue};
use reqwest::{Client, redirect};
use tracing::{debug, warn};

use crate::error::{Error, Result};
use crate::net_guard::FilteringResolver;
use crate::options::DownloadOptions;
use crate::proxy::Proxies;

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
    /// Resolves names for every client [`build`](Self::build) makes, when set
    /// (to keep a download away from the local network).
    resolver: Option<Arc<FilteringResolver>>,
    /// The proxy settings, read once, that the HTTP clients and the engine's
    /// own decisions both follow.
    proxies: Arc<Proxies>,
    /// Whether the HTTP clients go through [`proxies`](Self::proxies) too.
    apply_proxies: bool,
}

/// The process-wide TLS configuration: the trusted root certificates are read
/// once, and sessions resume across downloads from the same host too.
static SHARED_TLS: OnceLock<rustls::ClientConfig> = OnceLock::new();

impl Clients {
    /// Uses the operating system's root certificates (and more on Windows, D116), read once per process,
    /// and the proxy settings `opts` asks for, as they are now. The first call reads the
    /// certificate store, so it must not run on an async thread; see
    /// [`Clients::shared_ready`]. The proxy settings come from the environment
    /// and, on Windows, the current user's registry hive, which is in memory,
    /// so reading them waits on nothing (the HTTP client read the same values
    /// for every client it built).
    pub fn new(opts: &DownloadOptions) -> Result<Self> {
        let tls = if let Some(tls) = SHARED_TLS.get() {
            tls
        } else {
            let config = tls_config(trusted_roots())?;
            SHARED_TLS.get_or_init(|| config)
        };
        Ok(Self::with_config(
            opts,
            tls.clone(),
            Proxies::of(&opts.proxy),
        ))
    }

    /// Whether [`Clients::new`] would return without touching the disk.
    pub fn shared_ready() -> bool {
        SHARED_TLS.get().is_some()
    }

    /// The TLS configuration (root certificates included) other protocols
    /// start from.
    pub fn tls(&self) -> &rustls::ClientConfig {
        &self.tls
    }

    /// Uses the given root certificates and a session cache of its own, and
    /// no proxy whatever the machine sets, so a test behaves the same on
    /// every machine.
    #[cfg(test)]
    pub fn with_roots(opts: &DownloadOptions, roots: rustls::RootCertStore) -> Self {
        Self::with_config(opts, tls_config(roots).unwrap(), Proxies::default())
    }

    /// The same clients, deciding as if the environment were what `env`
    /// reads; their HTTP clients use no proxy, since a test's proxies do not
    /// exist.
    #[cfg(all(test, feature = "http3"))]
    pub fn with_environment(mut self, env: fn(&str) -> Option<std::ffi::OsString>) -> Self {
        self.proxies = Arc::new(Proxies::read(&env, || None));
        self.apply_proxies = false;
        self
    }

    /// The same clients, following `proxies` in their HTTP clients too.
    #[cfg(test)]
    pub fn with_proxies(mut self, proxies: Proxies) -> Self {
        self.proxies = Arc::new(proxies);
        self.apply_proxies = true;
        self
    }

    /// The proxy settings these clients follow.
    pub fn proxies(&self) -> &Proxies {
        &self.proxies
    }

    fn with_config(opts: &DownloadOptions, tls: rustls::ClientConfig, proxies: Proxies) -> Self {
        Self {
            tls,
            user_agent: opts.user_agent.clone(),
            connect_timeout: opts.connect_timeout,
            resolver: None,
            proxies: Arc::new(proxies),
            apply_proxies: true,
        }
    }

    /// Resolves names through `resolver` in every client built from now on.
    pub fn set_resolver(&mut self, resolver: Arc<FilteringResolver>) {
        self.resolver = Some(resolver);
    }

    /// A client with its own connection pool and the shared TLS session cache.
    pub fn build(&self) -> Result<Client> {
        let builder = match &self.resolver {
            Some(resolver) => self.builder().dns_resolver(resolver.clone()),
            None => self.builder(),
        };
        builder.build().map_err(Error::network)
    }

    /// The settings every client shares, for a caller that adds its own.
    pub fn builder(&self) -> reqwest::ClientBuilder {
        let mut headers = HeaderMap::new();
        // Compressed transfer encodings break byte offsets.
        headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        // Given a proxy, the HTTP client reads no setting of its own, so it
        // cannot differ from the engine's decisions, whatever reqwest
        // features another crate in the build turns on.
        let builder = if self.apply_proxies {
            Client::builder().proxy(self.proxies.client())
        } else {
            Client::builder().no_proxy()
        };
        builder
            // A clone shares the session cache: it lives behind an `Arc` in the config.
            .tls_backend_preconfigured(self.tls.clone())
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

/// The engine's TLS setup, built once: TLS 1.2 and 1.3, the process's crypto
/// provider or else ring, the given root certificates, HTTP/2 offered through ALPN.
fn tls_config(roots: rustls::RootCertStore) -> Result<rustls::ClientConfig> {
    let installed = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .and_then(|provider| {
            rustls::ClientConfig::builder_with_provider(provider)
                .with_protocol_versions(rustls::ALL_VERSIONS)
                .ok()
        });
    let builder = match installed {
        Some(builder) => builder,
        // The ring provider supports both versions, so this only fails if the
        // TLS library itself is broken.
        None => rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .map_err(|e| Error::Internal(format!("TLS 1.2 and 1.3 are not available: {e}")))?,
    };
    let mut config = builder.with_root_certificates(roots).with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

/// The operating system's trusted roots and, on Windows, the bundled ones (D116). Unreadable
/// entries are skipped; with none at all, HTTPS requests fail with a certificate error.
fn trusted_roots() -> rustls::RootCertStore {
    let loaded = rustls_native_certs::load_native_certs();
    for error in &loaded.errors {
        // Its message already includes its source.
        debug!(%error, "skipped part of the system certificate store");
    }
    let named_files = names_certificates(
        std::env::var_os("SSL_CERT_FILE").as_deref(),
        std::env::var_os("SSL_CERT_DIR").as_deref(),
    );
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(usable_roots(loaded.certs, named_files));
    if roots.is_empty() {
        warn!("no usable root certificates found; HTTPS downloads will fail");
    }
    roots
}

/// Whether these variables make `load_native_certs` read named files instead of the system's
/// store, as the bundled roots then do not apply either. It ignores empty entries of the folder list.
pub(super) fn names_certificates(file: Option<&OsStr>, folders: Option<&OsStr>) -> bool {
    file.is_some()
        || folders.is_some_and(|folders| {
            std::env::split_paths(folders).any(|folder| !folder.as_os_str().is_empty())
        })
}

/// The system's roots and, unless certificate files were named, the bundled ones, without what
/// Windows disallows. With the Disallowed stores unreadable, no bundled root can be judged.
#[cfg(windows)]
fn usable_roots(
    system: Vec<rustls::pki_types::CertificateDer<'static>>,
    named_files: bool,
) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    match disallowed_certificates() {
        Ok(disallowed) => {
            let bundled: &[_] = if named_files {
                &[]
            } else {
                webpki_root_certs::TLS_SERVER_ROOT_CERTS
            };
            roots_not_disallowed(system, bundled, &disallowed)
        }
        Err(error) => {
            warn!(
                %error,
                "could not read Windows' disallowed certificates; the bundled roots stay out"
            );
            system
        }
    }
}

/// Only Windows fetches roots on demand; elsewhere the system's store is the whole answer.
#[cfg(not(windows))]
fn usable_roots(
    system: Vec<rustls::pki_types::CertificateDer<'static>>,
    _named_files: bool,
) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    system
}

#[cfg(windows)]
pub(super) fn roots_not_disallowed(
    system: Vec<rustls::pki_types::CertificateDer<'static>>,
    bundled: &[rustls::pki_types::CertificateDer<'static>],
    disallowed: &[Vec<u8>],
) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    system
        .into_iter()
        .chain(bundled.iter().cloned())
        .filter(|root| !disallowed.iter().any(|der| der.as_slice() == root.as_ref()))
        .collect()
}

/// What Microsoft or an administrator told Windows never to trust.
#[cfg(windows)]
pub(super) fn disallowed_certificates() -> std::io::Result<Vec<Vec<u8>>> {
    use schannel::cert_store::CertStoreBuilder;

    // Read-only: the machine's store refuses anything more without administrator rights.
    let mut options = CertStoreBuilder::new();
    options.read_only();
    let mut found = Vec::new();
    for store in [
        options.open_current_user("Disallowed")?,
        options.open_local_machine("Disallowed")?,
    ] {
        found.extend(store.certs().map(|cert| cert.to_der().to_vec()));
    }
    Ok(found)
}
