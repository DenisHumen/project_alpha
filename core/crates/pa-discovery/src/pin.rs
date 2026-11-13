// SPDX-License-Identifier: MPL-2.0
//! Certificate pinning for our media server (SRV-004, ADR-NNNN).
//!
//! The server presents a self-signed certificate and announces its
//! fingerprint in the TXT entry `fp` of `_projectalpha._tcp`
//! ([`ServerTxt::fingerprint`](crate::ServerTxt::fingerprint)). The format is
//! `sha256:<64 lowercase hex>` of the DER leaf certificate, the same value as
//! `openssl x509 -noout -fingerprint -sha256` without colons, so a user can
//! compare it by hand.
//!
//! Trust on first use: the first fingerprint seen for a server id is stored
//! by the app ([`check_pin`] says when); afterwards the client connects only
//! with [`PinnedVerifier`] for the stored value. A different certificate at
//! the same address (a substituted server) fails the TLS handshake, and a
//! changed `fp` in the announcement is reported as [`PinCheck::Changed`]
//! instead of being trusted silently.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

/// Prefix of the only supported fingerprint algorithm.
pub const FINGERPRINT_PREFIX: &str = "sha256:";

/// SHA-256 fingerprint of a DER certificate.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

/// A TXT `fp` value that is not `sha256:<64 hex>`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("fingerprint {0:?} is not sha256:<64 hex digits>")]
pub struct FingerprintError(pub String);

impl Fingerprint {
    /// Fingerprint of a DER-encoded certificate.
    pub fn of_der(der: &[u8]) -> Self {
        Self(Sha256::digest(der).into())
    }

    /// Raw digest.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(FINGERPRINT_PREFIX)?;
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl FromStr for Fingerprint {
    type Err = FingerprintError;

    /// Accepts `sha256:` followed by 64 hex digits, any case; colons between
    /// bytes (as printed by OpenSSL) are allowed.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || FingerprintError(s.to_owned());
        let prefix = s.get(..FINGERPRINT_PREFIX.len()).ok_or_else(err)?;
        if !prefix.eq_ignore_ascii_case(FINGERPRINT_PREFIX) {
            return Err(err());
        }
        let hex: Vec<u8> = s[FINGERPRINT_PREFIX.len()..]
            .bytes()
            .filter(|&b| b != b':')
            .collect();
        if hex.len() != 64 {
            return Err(err());
        }
        let mut out = [0u8; 32];
        for (byte, pair) in out.iter_mut().zip(hex.chunks_exact(2)) {
            let hi = (pair[0] as char).to_digit(16).ok_or_else(err)?;
            let lo = (pair[1] as char).to_digit(16).ok_or_else(err)?;
            *byte = (hi * 16 + lo) as u8;
        }
        Ok(Self(out))
    }
}

/// Outcome of comparing an announced fingerprint with the stored pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinCheck {
    /// No pin for this server id yet: store the announced one (TOFU).
    FirstUse,
    /// The announcement matches the pin.
    Match,
    /// The server announces another certificate. Do not connect with the
    /// new value: ask the user (the server was reinstalled, or someone
    /// impersonates it); keep connecting with `pinned` only.
    Changed {
        /// The stored pin.
        pinned: Fingerprint,
    },
}

/// Trust-on-first-use decision for a server whose stored pin is `pinned`.
pub fn check_pin(pinned: Option<Fingerprint>, announced: Fingerprint) -> PinCheck {
    match pinned {
        None => PinCheck::FirstUse,
        Some(pin) if pin == announced => PinCheck::Match,
        Some(pin) => PinCheck::Changed { pinned: pin },
    }
}

/// rustls verifier that accepts exactly one server certificate.
///
/// The leaf certificate must have the pinned fingerprint; the host name,
/// the issuer and the validity dates are not checked (the certificate is
/// self-signed and identified by its hash). Handshake signatures are still
/// verified, so only the holder of the private key passes.
#[derive(Debug)]
pub struct PinnedVerifier {
    pin: Fingerprint,
    provider: Arc<CryptoProvider>,
}

impl PinnedVerifier {
    /// Verifier for `pin` with the signature algorithms of `provider`.
    pub fn new(pin: Fingerprint, provider: Arc<CryptoProvider>) -> Self {
        Self { pin, provider }
    }

    /// Verifier for `pin` with the `ring` provider.
    pub fn with_ring(pin: Fingerprint) -> Self {
        Self::new(pin, Arc::new(rustls::crypto::ring::default_provider()))
    }

    /// The pinned fingerprint.
    pub fn pin(&self) -> Fingerprint {
        self.pin
    }

    /// Client configuration (no client certificate) that trusts only `pin`;
    /// ALPN offers HTTP/1.1.
    pub fn client_config(pin: Fingerprint) -> Result<rustls::ClientConfig, rustls::Error> {
        let verifier = Self::with_ring(pin);
        let provider = Arc::clone(&verifier.provider);
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(config)
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if Fingerprint::of_der(end_entity) == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let fp = Fingerprint::of_der(b"certificate");
        let text = fp.to_string();
        assert!(text.starts_with("sha256:"));
        assert_eq!(text.len(), 7 + 64);
        assert_eq!(text.parse::<Fingerprint>().unwrap(), fp);
        // OpenSSL style: upper case with colons.
        let openssl = fp
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":");
        assert_eq!(format!("SHA256:{openssl}").parse::<Fingerprint>().unwrap(), fp);
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "sha256:",
            "sha1:00",
            "sha256:abc",
            &format!("sha256:{}", "g".repeat(64)),
            &format!("sha256:{}", "0".repeat(66)),
            &format!("md5:{}", "0".repeat(64)),
        ] {
            assert!(bad.parse::<Fingerprint>().is_err(), "{bad}");
        }
    }

    #[test]
    fn tofu_decisions() {
        let a = Fingerprint::of_der(b"a");
        let b = Fingerprint::of_der(b"b");
        assert_eq!(check_pin(None, a), PinCheck::FirstUse);
        assert_eq!(check_pin(Some(a), a), PinCheck::Match);
        assert_eq!(check_pin(Some(a), b), PinCheck::Changed { pinned: a });
    }

    #[test]
    fn verifier_checks_only_the_leaf_hash() {
        let leaf = CertificateDer::from(b"leaf".to_vec());
        let other = CertificateDer::from(b"other".to_vec());
        let v = PinnedVerifier::with_ring(Fingerprint::of_der(&leaf));
        let name = ServerName::try_from("anything.local").unwrap();
        assert!(
            v.verify_server_cert(&leaf, &[], &name, &[], UnixTime::now())
                .is_ok()
        );
        assert!(
            v.verify_server_cert(&other, &[], &name, &[], UnixTime::now())
                .is_err()
        );
        assert!(!v.supported_verify_schemes().is_empty());
    }
}
