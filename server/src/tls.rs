// SPDX-License-Identifier: MPL-2.0
//! HTTPS with rustls (SRV-004, ADR-NNNN).
//!
//! The server identity is a certificate and key in PEM files: either set in
//! `[tls]` or a self-signed ECDSA P-256 pair created on first start in
//! `data_dir/tls/`. Clients do not validate it against a CA: they pin its
//! [`Fingerprint`] (SHA-256 of the DER leaf), which the server announces in
//! the mDNS TXT entry `fp`. Keeping the files keeps the fingerprint;
//! deleting them makes a new one, and paired clients will refuse it until
//! the user confirms (TOFU, `pa_discovery::check_pin`).
//!
//! [`TlsListener`] makes TLS handshakes off the accept loop (one task per
//! connection, bounded and with a timeout), so a slow or silent client does
//! not hold up everybody else.

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pa_discovery::Fingerprint;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::Error;

/// Directory with the generated pair inside the data directory.
pub const TLS_DIR: &str = "tls";
/// Generated certificate file name.
pub const CERT_FILE: &str = "server.crt";
/// Generated private key file name.
pub const KEY_FILE: &str = "server.key";

/// Common name of the generated certificate.
const COMMON_NAME: &str = "Kinolune media server";
/// A client must finish the handshake within this time.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Handshakes in progress at once; more connections wait in the backlog.
const MAX_HANDSHAKES: usize = 256;

fn tls_err(context: impl Into<String>, e: impl std::fmt::Display) -> Error {
    Error::Tls(format!("{}: {e}", context.into()))
}

/// Certificate chain and key of the server.
pub struct TlsIdentity {
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    fingerprint: Fingerprint,
    cert_path: PathBuf,
}

impl std::fmt::Debug for TlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsIdentity")
            .field("fingerprint", &self.fingerprint)
            .field("cert_path", &self.cert_path)
            .finish_non_exhaustive()
    }
}

impl TlsIdentity {
    /// Loads `cert`/`key` if both are set; otherwise loads or creates the
    /// self-signed pair in `data_dir/tls`. `host` (the mDNS host name,
    /// `pa-….local.`) goes into the subject alternative names of a new
    /// certificate.
    pub fn load_or_create(
        cert: Option<&Path>,
        key: Option<&Path>,
        data_dir: &Path,
        host: &str,
    ) -> Result<Self, Error> {
        match (cert, key) {
            (Some(cert), Some(key)) => Self::load(cert, key),
            (None, None) => {
                let dir = data_dir.join(TLS_DIR);
                let (cert, key) = (dir.join(CERT_FILE), dir.join(KEY_FILE));
                if !(cert.exists() && key.exists()) {
                    generate(&dir, &cert, &key, host)?;
                    tracing::info!(cert = %cert.display(), "created a self-signed certificate");
                }
                Self::load(&cert, &key)
            }
            _ => Err(Error::Config(
                "tls: set both cert and key, or neither".to_owned(),
            )),
        }
    }

    /// Loads a PEM chain (leaf first) and a PEM private key.
    pub fn load(cert: &Path, key: &Path) -> Result<Self, Error> {
        let certs = CertificateDer::pem_file_iter(cert)
            .and_then(|iter| iter.collect::<Result<Vec<_>, _>>())
            .map_err(|e| tls_err(cert.display().to_string(), e))?;
        let leaf = certs
            .first()
            .ok_or_else(|| tls_err(cert.display().to_string(), "no certificate"))?;
        let fingerprint = Fingerprint::of_der(leaf);
        let key =
            PrivateKeyDer::from_pem_file(key).map_err(|e| tls_err(key.display().to_string(), e))?;
        Ok(Self {
            certs,
            key,
            fingerprint,
            cert_path: cert.to_owned(),
        })
    }

    /// Fingerprint of the leaf certificate (TXT `fp`).
    pub fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// File of the certificate.
    pub fn cert_path(&self) -> &Path {
        &self.cert_path
    }

    /// rustls configuration: TLS 1.2 and 1.3, ring provider, ALPN
    /// `http/1.1`, no client certificates.
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| tls_err("protocol versions", e))?
            .with_no_client_auth()
            .with_single_cert(self.certs.clone(), self.key.clone_key())
            .map_err(|e| tls_err(self.cert_path.display().to_string(), e))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

/// Writes a new self-signed certificate and key. The key is written first
/// and only readable by the owner.
fn generate(dir: &Path, cert: &Path, key: &Path, host: &str) -> Result<(), Error> {
    let io_err = |context: &Path, source: io::Error| Error::Io {
        context: context.display().to_string(),
        source,
    };
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let mut names = vec!["localhost".to_owned()];
    let host = host.trim_end_matches('.');
    if !host.is_empty() {
        names.push(host.to_owned());
    }
    let mut params = rcgen::CertificateParams::new(names).map_err(|e| tls_err("certificate", e))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, COMMON_NAME);
    let pair = rcgen::KeyPair::generate().map_err(|e| tls_err("key", e))?;
    let certificate = params
        .self_signed(&pair)
        .map_err(|e| tls_err("certificate", e))?;
    write_atomic(key, pair.serialize_pem().as_bytes(), true).map_err(|e| io_err(key, e))?;
    write_atomic(cert, certificate.pem().as_bytes(), false).map_err(|e| io_err(cert, e))
}

/// Writes through a temporary file and a rename, so a crash never leaves a
/// half-written file behind. `private`: owner-only permissions on Unix.
fn write_atomic(path: &Path, data: &[u8], private: bool) -> io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(&tmp)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

/// TLS connections for `axum::serve`: a background task accepts TCP and
/// runs handshakes; finished ones come out of [`accept`](axum::serve::Listener::accept).
pub struct TlsListener {
    ready: mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local_addr: SocketAddr,
}

impl TlsListener {
    /// Starts accepting on `listener`. The returned task owns the socket:
    /// abort it to close the port.
    pub fn start(
        listener: TcpListener,
        config: Arc<rustls::ServerConfig>,
    ) -> io::Result<(Self, JoinHandle<()>)> {
        let local_addr = listener.local_addr()?;
        let (tx, ready) = mpsc::channel(64);
        let acceptor = TlsAcceptor::from(config);
        let permits = Arc::new(Semaphore::new(MAX_HANDSHAKES));
        let task = tokio::spawn(async move {
            loop {
                let Ok(permit) = Arc::clone(&permits).acquire_owned().await else {
                    return;
                };
                let (tcp, peer) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        // EMFILE and friends: back off instead of spinning.
                        tracing::warn!(error = %e, "accept failed");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        continue;
                    }
                };
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await {
                        Ok(Ok(stream)) => {
                            let _ = tx.send((stream, peer)).await;
                        }
                        Ok(Err(e)) => tracing::debug!(%peer, error = %e, "TLS handshake failed"),
                        Err(_) => tracing::debug!(%peer, "TLS handshake timed out"),
                    }
                });
            }
        });
        Ok((Self { ready, local_addr }, task))
    }
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(conn) => conn,
            // The accept task is gone (shutdown): nothing more will come.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_then_reuses() {
        let tmp = tempfile::tempdir().unwrap();
        let first = TlsIdentity::load_or_create(None, None, tmp.path(), "pa-abc.local.").unwrap();
        let second = TlsIdentity::load_or_create(None, None, tmp.path(), "pa-abc.local.").unwrap();
        assert_eq!(first.fingerprint(), second.fingerprint());
        assert!(first.server_config().is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key = tmp.path().join(TLS_DIR).join(KEY_FILE);
            let mode = std::fs::metadata(key).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Explicit files: the same pair loaded by path.
        let dir = tmp.path().join(TLS_DIR);
        let explicit = TlsIdentity::load_or_create(
            Some(&dir.join(CERT_FILE)),
            Some(&dir.join(KEY_FILE)),
            Path::new("/nonexistent"),
            "",
        )
        .unwrap();
        assert_eq!(explicit.fingerprint(), first.fingerprint());
    }

    #[test]
    fn rejects_half_config_and_bad_files() {
        let tmp = tempfile::tempdir().unwrap();
        let cert = tmp.path().join("c.pem");
        assert!(matches!(
            TlsIdentity::load_or_create(Some(&cert), None, tmp.path(), ""),
            Err(Error::Config(_))
        ));
        std::fs::write(&cert, "not a pem").unwrap();
        assert!(matches!(
            TlsIdentity::load(&cert, &cert),
            Err(Error::Tls(_))
        ));
    }
}
