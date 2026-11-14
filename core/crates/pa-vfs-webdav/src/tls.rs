// SPDX-License-Identifier: MPL-2.0
//! TLS trust: the platform verifier, or one pinned certificate (TOFU).
//!
//! A NAS usually serves WebDAV over HTTPS with a self-signed certificate.
//! The app first connects with [`TlsTrust::System`]; if that fails with
//! [`is_untrusted_certificate`], it shows the fingerprint from
//! [`fetch_fingerprint`] to the user and, after confirmation, stores it with
//! the source and connects with [`TlsTrust::Pinned`]. From then on only that
//! exact certificate is accepted (`pa_discovery::PinnedVerifier`, the
//! same verifier and `sha256:` format as for our own server, SRV-004).

use std::error::Error as StdError;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pa_discovery::{Fingerprint, PinnedVerifier};
use pa_vfs::{ErrorKind, Result, VfsError};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

/// Which server certificates are accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsTrust {
    /// Certificates signed by a CA the platform trusts (default).
    #[default]
    System,
    /// Exactly the certificate with this SHA-256 fingerprint; host name,
    /// issuer and validity dates are not checked (self-signed NAS).
    Pinned(Fingerprint),
}

/// Marker in the error chain of a [`VfsError`] caused by a certificate the
/// configured [`TlsTrust`] rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedCertificate(pub String);

impl std::fmt::Display for UntrustedCertificate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "untrusted TLS certificate: {}", self.0)
    }
}

impl StdError for UntrustedCertificate {}

/// `true` if `err` failed because the server certificate is not trusted:
/// offer the user to pin it (see the module docs).
#[must_use]
pub fn is_untrusted_certificate(err: &VfsError) -> bool {
    err.source()
        .is_some_and(|s| s.downcast_ref::<UntrustedCertificate>().is_some())
}

/// Finds a rustls certificate error anywhere in a reqwest error chain.
/// `io::Error` hides its payload from `source()`, so it is unwrapped too.
pub(crate) fn certificate_error(err: &(dyn StdError + 'static)) -> Option<String> {
    let mut cur: Option<&(dyn StdError + 'static)> = Some(err);
    while let Some(e) = cur {
        if let Some(r) = e.downcast_ref::<rustls::Error>() {
            return is_cert(r).then(|| r.to_string());
        }
        // hyper and tokio-rustls nest `io::Error`s in each other.
        cur = match e.downcast_ref::<io::Error>().and_then(io::Error::get_ref) {
            Some(inner) => Some(inner as &(dyn StdError + 'static)),
            None => e.source(),
        };
    }
    None
}

fn is_cert(e: &rustls::Error) -> bool {
    matches!(
        e,
        rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented
    )
}

/// The ring provider for every TLS config of this crate.
pub(crate) fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Client config for [`TlsTrust::Pinned`].
pub(crate) fn pinned_config(pin: Fingerprint) -> Result<rustls::ClientConfig> {
    PinnedVerifier::client_config(pin)
        .map_err(|e| VfsError::new(ErrorKind::Other, format!("TLS config: {e}")))
}

/// Connects to `url`'s host, completes enough of a TLS handshake to see the
/// server certificate, and returns its fingerprint. No HTTP request and no
/// credentials are sent. Use it to show the user what to pin.
pub fn fetch_fingerprint(url: &str, timeout: Duration) -> Result<Fingerprint> {
    let base = crate::href::Base::parse(url)?;
    let url = base.url();
    if url.scheme() != "https" {
        return Err(VfsError::new(ErrorKind::InvalidPath, "not an https URL"));
    }
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_matches(['[', ']'])
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(443);
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| VfsError::from_io(e, &host))?
        .next()
        .ok_or_else(|| VfsError::new(ErrorKind::Unavailable, format!("{host}: no address")))?;
    let mut tcp =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| VfsError::from_io(e, &host))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| VfsError::from_io(e, &host))?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|e| VfsError::from_io(e, &host))?;

    let seen = Arc::new(Mutex::new(None));
    let verifier = Capture {
        seen: Arc::clone(&seen),
        provider: provider(),
    };
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| VfsError::new(ErrorKind::Other, e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    let name = ServerName::try_from(host.clone())
        .map_err(|e| VfsError::new(ErrorKind::InvalidPath, format!("{host}: {e}")))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name)
        .map_err(|e| VfsError::new(ErrorKind::Other, e.to_string()))?;
    // The verifier aborts the handshake right after it has seen the
    // certificate, so this loop ends with an error in the normal case.
    while conn.is_handshaking() {
        if conn.complete_io(&mut tcp).is_err() {
            break;
        }
    }
    let fp = *seen.lock().unwrap_or_else(PoisonError::into_inner);
    fp.ok_or_else(|| {
        VfsError::new(
            ErrorKind::Unavailable,
            format!("{host}: no TLS certificate received"),
        )
    })
}

/// Records the leaf certificate and refuses it.
#[derive(Debug)]
struct Capture {
    seen: Arc<Mutex<Option<Fingerprint>>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for Capture {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(Fingerprint::of_der(end_entity));
        Err(rustls::Error::InvalidCertificate(
            rustls::CertificateError::ApplicationVerificationFailure,
        ))
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("fingerprint probe only".into()))
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("fingerprint probe only".into()))
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
