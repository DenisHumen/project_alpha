// SPDX-License-Identifier: MPL-2.0
//! TLS 1.3 channel of LAN sync, bound to the pairing keys.
//!
//! Each node makes an ephemeral self-signed certificate at start (never
//! stored) and accepts any peer certificate, still verifying the handshake
//! signature. Who is at the other end is proven by the sync handshake: the
//! `hello.nonce` of each side is not random but a TLS exporter value of this
//! very connection (RFC 5705/8446 §7.5, channel binding like RFC 9266), and
//! `auth` signs both nonces with the Ed25519 key from pairing. A relay in
//! the middle has two TLS sessions with different exporters, so the nonces
//! it forwards do not match and the signatures it forwards do not verify.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ConnectionCommon, DigitallySignedStruct, SignatureScheme};

/// Exporter label of the `hello.nonce`.
pub const NONCE_LABEL: &[u8] = b"EXPORTER-kinolune-sync-nonce";
/// Name in the certificate and SNI; not checked by the peer.
pub const SERVER_NAME: &str = "kinolune-sync";

/// Which end of the TLS connection a side is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsSide {
    /// Dialed the connection.
    Client,
    /// Accepted it.
    Server,
}

impl TlsSide {
    fn context(self) -> &'static [u8] {
        match self {
            Self::Client => b"client",
            Self::Server => b"server",
        }
    }

    /// The other end.
    pub fn peer(self) -> Self {
        match self {
            Self::Client => Self::Server,
            Self::Server => Self::Client,
        }
    }
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// TLS error as an I/O error (what the connection code deals in).
pub fn io_err(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

/// The `hello.nonce` of `side` on this connection: 32 exporter bytes, hex.
pub fn channel_nonce<D>(conn: &ConnectionCommon<D>, side: TlsSide) -> Result<String, rustls::Error> {
    let out = conn.export_keying_material([0u8; 32], NONCE_LABEL, Some(side.context()))?;
    Ok(out.iter().map(|b| format!("{b:02x}")).collect())
}

/// Server configuration with a fresh self-signed certificate.
pub fn server_config() -> Result<rustls::ServerConfig, std::io::Error> {
    let pair = rcgen::KeyPair::generate().map_err(io_err)?;
    let cert = rcgen::CertificateParams::new(vec![SERVER_NAME.to_owned()])
        .map_err(io_err)?
        .self_signed(&pair)
        .map_err(io_err)?;
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pair.serialize_der()));
    rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io_err)?
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key)
        .map_err(io_err)
}

/// Client configuration that takes any certificate (see the module docs).
pub fn client_config() -> Result<rustls::ClientConfig, std::io::Error> {
    let provider = provider();
    Ok(rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(io_err)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert(provider)))
        .with_no_client_auth())
}

/// The server name to dial with.
pub fn server_name() -> ServerName<'static> {
    ServerName::try_from(SERVER_NAME).expect("valid DNS name")
}

/// Accepts any certificate but checks that the peer holds its key; identity
/// comes from the pairing handshake bound to the exporter.
#[derive(Debug)]
struct AnyCert(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not offered".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
