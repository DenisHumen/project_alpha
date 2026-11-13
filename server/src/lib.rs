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
//! [`Server::bind`] opens the databases and the socket; [`Server::run`]
//! serves until the shutdown future resolves, lets in-flight requests finish,
//! closes sync connections and checkpoints the databases.

pub mod config;
mod http;
pub mod logging;
pub mod sync;

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
}

impl Server {
    /// Opens `data_dir/library.sqlite3` and `data_dir/sync.sqlite3` and
    /// binds `config.listen`.
    pub async fn bind(config: &Config, data_dir: &Path) -> Result<Self, Error> {
        let path = data_dir.join(config::LIBRARY_FILE);
        let journal = data_dir.join(sync::JOURNAL_FILE);
        let (library, hub) = {
            let path = path.clone();
            let sync_config = config.sync.clone();
            tokio::task::spawn_blocking(move || {
                let library = Library::open(&path)?;
                let hub = sync::Hub::open(&journal, library.clone(), &sync_config)?;
                Ok::<_, Error>((library, hub))
            })
            .await
            .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))?
        };
        let listener = TcpListener::bind(config.listen)
            .await
            .map_err(|source| Error::Io {
                context: format!("bind {}", config.listen),
                source,
            })?;
        tracing::info!(
            db = %path.display(),
            schema = pa_library::latest_version(),
            "library database ready"
        );
        tracing::info!(hub = hub.hub_id(), path = sync::PATH, "sync hub ready");
        Ok(Self {
            listener,
            library,
            hub,
        })
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

    /// Serves until `shutdown` resolves, then waits for in-flight requests,
    /// closes sync connections and checkpoints the databases.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), Error> {
        let addr = self.local_addr()?;
        tracing::info!(%addr, version = VERSION, "listening");
        let app = http::router(self.library.clone(), self.hub.clone());
        let hub = self.hub.clone();
        let shutdown = async move {
            shutdown.await;
            // WebSocket connections are not tracked by the graceful
            // shutdown of axum: close them ourselves.
            hub.close();
        };
        axum::serve(self.listener, app)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|source| Error::Io {
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
