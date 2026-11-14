// SPDX-License-Identifier: MPL-2.0
//! Kinolune media server (mode B, `docs/research/04` §1.5).
//!
//! One binary on top of the same core crates as the apps: `pa-runtime` for
//! the tokio runtime, data directories and settings, `pa-library` for the
//! library database (SQLite WAL, same schema and migrations as the client).
//! This crate adds the HTTP layer (axum), configuration, structured logs,
//! graceful shutdown and the sync hub ([`sync`], WebSocket at
//! [`sync::PATH`]). The REST API comes with SRV-002 and later tasks.
//!
//! Transport (SRV-004, ADR-NNNN): HTTPS by default with a self-signed
//! certificate ([`tls`]) whose fingerprint clients pin; the server announces
//! itself on mDNS as `_projectalpha._tcp` with that fingerprint in TXT `fp`
//! ([`announce`]). Plain HTTP only with `[tls] enabled = false`.
//!
//! [`Server::bind`] opens the databases, loads or creates the certificate and
//! binds the socket; [`Server::run`] announces the server, serves until the
//! shutdown future resolves, lets in-flight requests finish, withdraws the
//! announcement, closes sync connections and checkpoints the databases.

pub mod announce;
pub mod config;
mod http;
pub mod logging;
pub mod pairing;
pub mod sync;
pub mod tls;

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pa_discovery::{Fingerprint, Interfaces, MdnsConfig};
use pa_library::rusqlite::Connection;
use tokio::net::TcpListener;

pub use config::Config;

/// Server version reported by `/health`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Service name reported by `/health` (same as the NAS OS unit).
pub const SERVICE: &str = "alpha-media";

/// Error of server start-up and shutdown.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid configuration file or environment variable.
    #[error("config: {0}")]
    Config(String),
    /// Core runtime or data directories.
    #[error(transparent)]
    Runtime(#[from] pa_runtime::RuntimeError),
    /// The library database could not be opened or migrated.
    #[error("library database {path}: {source}")]
    Library {
        /// Database file.
        path: PathBuf,
        /// Cause.
        source: pa_library::Error,
    },
    /// The sync journal could not be opened.
    #[error("sync journal {path}: {source}")]
    Sync {
        /// Database file.
        path: PathBuf,
        /// Cause.
        source: sync::journal::JournalError,
    },
    /// Certificate or key could not be loaded, created or used.
    #[error("tls: {0}")]
    Tls(String),
    /// The device family (keys, roster) could not be loaded.
    #[error("device family in {path}: {source}")]
    Pairing {
        /// Data directory.
        path: PathBuf,
        /// Cause.
        source: pa_pairing::Error,
    },
    /// Socket or other I/O error.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// Cause.
        source: std::io::Error,
    },
}

/// The library database shared by request handlers. SQLite allows one
/// writer; the mutex serializes access, and handlers run queries in
/// `spawn_blocking` so the async workers never wait on it.
#[derive(Clone)]
pub struct Library {
    conn: Arc<Mutex<Connection>>,
}

impl Library {
    /// Opens (creating and migrating if needed) the database file.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let conn = pa_library::open(path).map_err(|source| Error::Library {
            path: path.to_owned(),
            source,
        })?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` with the connection on the blocking thread pool.
    pub async fn with<T, F>(&self, f: F) -> Result<T, pa_library::rusqlite::Error>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, pa_library::rusqlite::Error> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&conn)
        })
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))
    }

    /// Newest applied schema version.
    pub async fn schema_version(&self) -> Result<i64, pa_library::rusqlite::Error> {
        self.with(|conn| {
            conn.query_row(
                &format!(
                    "SELECT coalesce(max(version), 0) FROM {}",
                    pa_library::HISTORY_TABLE
                ),
                [],
                |r| r.get(0),
            )
        })
        .await
    }

    /// Moves the WAL into the main file so a stopped server leaves one
    /// self-contained database (backup = copy of the file).
    pub async fn checkpoint(&self) -> Result<(), pa_library::rusqlite::Error> {
        self.with(|conn| conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())))
            .await
    }
}

/// A server with an open database and a bound socket, not yet serving.
pub struct Server {
    listener: TcpListener,
    library: Library,
    hub: sync::Hub,
    tls: Option<tls::TlsIdentity>,
    discovery: config::DiscoveryConfig,
    mdns: MdnsConfig,
}

impl Server {
    /// Opens `data_dir/library.sqlite3` and `data_dir/sync.sqlite3`, loads
    /// or creates the TLS certificate (unless TLS is off) and binds
    /// `config.listen`.
    pub async fn bind(config: &Config, data_dir: &Path) -> Result<Self, Error> {
        let path = data_dir.join(config::LIBRARY_FILE);
        let journal = data_dir.join(sync::JOURNAL_FILE);
        let listener = TcpListener::bind(config.listen)
            .await
            .map_err(|source| Error::Io {
                context: format!("bind {}", config.listen),
                source,
            })?;
        // Pairing QR codes carry the actual address, when it is a concrete one.
        let addr = listener
            .local_addr()
            .ok()
            .filter(|a| !a.ip().is_unspecified())
            .map(|a| a.to_string());
        let (library, hub, tls) = {
            let path = path.clone();
            let sync_config = config.sync.clone();
            let tls_config = config.tls.clone();
            let data_dir = data_dir.to_owned();
            tokio::task::spawn_blocking(move || {
                let library = Library::open(&path)?;
                let hub =
                    sync::Hub::open(&journal, library.clone(), &sync_config, &data_dir, addr)?;
                // The server id is the hub id: one stable UUID per server.
                let tls = tls_config
                    .enabled
                    .then(|| {
                        tls::TlsIdentity::load_or_create(
                            tls_config.cert.as_deref(),
                            tls_config.key.as_deref(),
                            &data_dir,
                            &pa_discovery::host_for_id(hub.hub_id()),
                        )
                    })
                    .transpose()?;
                if let Some(identity) = &tls {
                    // Pairing QR codes carry the fingerprint to pin.
                    hub.pairing()
                        .set_fingerprint(identity.fingerprint().to_string());
                }
                Ok::<_, Error>((library, hub, tls))
            })
            .await
            .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))?
        };
        tracing::info!(
            db = %path.display(),
            schema = pa_library::latest_version(),
            "library database ready"
        );
        tracing::info!(hub = hub.hub_id(), path = sync::PATH, "sync hub ready");
        match &tls {
            Some(identity) => tracing::info!(
                fingerprint = %identity.fingerprint(),
                cert = %identity.cert_path().display(),
                "TLS ready"
            ),
            None => tracing::warn!("TLS is off: serving plain HTTP"),
        }
        Ok(Self {
            listener,
            library,
            hub,
            tls,
            discovery: config.discovery.clone(),
            mdns: MdnsConfig::default(),
        })
    }

    /// Replaces the mDNS settings of the announcement (tests: loopback and
    /// a private port). With [`Interfaces::LoopbackOnly`] the server is
    /// announced even when it listens on loopback.
    pub fn with_mdns(mut self, mdns: MdnsConfig) -> Self {
        self.mdns = mdns;
        self
    }

    /// Stable server id (TXT `id`), the same as the sync hub id.
    pub fn server_id(&self) -> &str {
        self.hub.hub_id()
    }

    /// Fingerprint of the TLS certificate (TXT `fp`); `None` with TLS off.
    pub fn fingerprint(&self) -> Option<Fingerprint> {
        self.tls.as_ref().map(tls::TlsIdentity::fingerprint)
    }

    /// Actual address (useful with port 0).
    pub fn local_addr(&self) -> Result<SocketAddr, Error> {
        self.listener.local_addr().map_err(|source| Error::Io {
            context: "local address".to_owned(),
            source,
        })
    }

    /// The library database.
    pub fn library(&self) -> &Library {
        &self.library
    }

    /// The sync hub.
    pub fn hub(&self) -> &sync::Hub {
        &self.hub
    }

    /// Starts the mDNS announcement unless it is off or pointless (loopback
    /// address). Failures are logged, not fatal: the server is still
    /// reachable by address.
    async fn announce(&self, addr: SocketAddr) -> Option<announce::Announcer> {
        if !self.discovery.enabled {
            tracing::info!("mDNS announcement is off");
            return None;
        }
        if addr.ip().is_loopback() && self.mdns.interfaces != Interfaces::LoopbackOnly {
            tracing::info!(%addr, "not announcing on mDNS: listening on loopback only");
            return None;
        }
        let name = announce::display_name(self.discovery.name.as_deref());
        let id = self.server_id().to_owned();
        let fingerprint = self.fingerprint();
        let mdns = self.mdns.clone();
        let started = tokio::task::spawn_blocking(move || {
            announce::Announcer::start(&mdns, &name, &id, addr.port(), fingerprint)
        })
        .await
        .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
        match started {
            Ok(announcer) => {
                tracing::info!(service = announcer.fullname(), "announced on mDNS");
                Some(announcer)
            }
            Err(e) => {
                tracing::warn!(error = %e, "mDNS announcement failed");
                None
            }
        }
    }

    /// Serves until `shutdown` resolves, then waits for in-flight requests,
    /// withdraws the mDNS announcement, closes sync connections and
    /// checkpoints the databases.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), Error> {
        let addr = self.local_addr()?;
        let announcer = self.announce(addr).await;
        let scheme = if self.tls.is_some() { "https" } else { "http" };
        tracing::info!(%addr, scheme, version = VERSION, "listening");
        let app = http::router(self.library.clone(), self.hub.clone());
        let hub = self.hub.clone();
        let shutdown = async move {
            shutdown.await;
            // WebSocket connections are not tracked by the graceful
            // shutdown of axum: close them ourselves.
            hub.close();
        };
        let served = match &self.tls {
            Some(identity) => {
                let config = identity.server_config()?;
                let (listener, accept_task) = tls::TlsListener::start(self.listener, config)
                    .map_err(|source| Error::Io {
                        context: "TLS listener".to_owned(),
                        source,
                    })?;
                let served = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<http::PeerAddr>(),
                )
                .with_graceful_shutdown(shutdown)
                .await;
                // The accept task owns the socket: close the port now.
                accept_task.abort();
                let _ = accept_task.await;
                served
            }
            None => {
                axum::serve(
                    self.listener,
                    app.into_make_service_with_connect_info::<http::PeerAddr>(),
                )
                .with_graceful_shutdown(shutdown)
                .await
            }
        };
        if let Some(announcer) = announcer {
            tokio::task::spawn_blocking(move || announcer.stop())
                .await
                .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
        }
        served.map_err(|source| Error::Io {
            context: "serve".to_owned(),
            source,
        })?;
        tracing::info!("stopped accepting connections");
        self.hub.drain(Duration::from_secs(5)).await;
        if let Err(e) = self.library.checkpoint().await {
            tracing::warn!(error = %e, "library checkpoint failed");
        }
        Ok(())
    }
}

/// Resolves on Ctrl-C or, on Unix, SIGTERM (systemd stop).
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "cannot listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!(signal = "ctrl-c", "shutting down"),
        () = terminate => tracing::info!(signal = "sigterm", "shutting down"),
    }
}
