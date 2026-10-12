//! A self-signed certificate for the TLS test servers.

use std::sync::Arc;

use base64::Engine as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

/// How many base64 characters a PEM line holds (RFC 7468).
const PEM_LINE: usize = 64;

/// A self-signed certificate for `127.0.0.1` and `localhost`, with what a
/// server needs to present it and what a client needs to trust it.
pub struct TestCertificate {
    chain: Vec<CertificateDer<'static>>,
    key: PrivatePkcs8KeyDer<'static>,
    roots: rustls::RootCertStore,
}

impl TestCertificate {
    /// Generates a new certificate and key.
    #[must_use]
    pub fn new() -> Self {
        let generated =
            rcgen::generate_simple_self_signed(vec!["127.0.0.1".into(), "localhost".into()])
                .unwrap();
        let der = CertificateDer::from(generated.cert.der().to_vec());
        let mut roots = rustls::RootCertStore::empty();
        roots.add(der.clone()).unwrap();
        Self {
            chain: vec![der],
            key: PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der()),
            roots,
        }
    }

    /// A root store that trusts only this certificate, for the client side.
    #[must_use]
    pub fn roots(&self) -> rustls::RootCertStore {
        self.roots.clone()
    }

    /// The certificate in PEM, for a file `SSL_CERT_FILE` can name.
    #[must_use]
    pub fn pem(&self) -> String {
        let mut pem = String::new();
        for der in &self.chain {
            pem.push_str("-----BEGIN CERTIFICATE-----\n");
            let encoded = base64::engine::general_purpose::STANDARD.encode(der);
            for line in encoded.as_bytes().chunks(PEM_LINE) {
                pem.push_str(std::str::from_utf8(line).unwrap());
                pem.push('\n');
            }
            pem.push_str("-----END CERTIFICATE-----\n");
        }
        pem
    }

    /// A server configuration presenting this certificate over TLS 1.2 and
    /// 1.3, offering the `alpn` protocols.
    #[must_use]
    pub fn server_config(&self, alpn: &[&[u8]]) -> rustls::ServerConfig {
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

impl Default for TestCertificate {
    fn default() -> Self {
        Self::new()
    }
}
