// SPDX-License-Identifier: MPL-2.0
//! Pool of libsmb2 contexts. A context serves one request at a time, so
//! parallel listing, reads and prefetch each take their own connection.

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use pa_smb2::{Context, FileId, Smb2Lib};
use pa_vfs::{ErrorKind, Result, VfsError};

use crate::config::SmbConfig;

/// Open files kept per connection; older handles are closed.
const FILES_PER_CONNECTION: usize = 16;

/// One connected context and the files it has open.
pub(crate) struct Conn {
    pub(crate) ctx: Context,
    files: HashMap<String, FileId>,
    order: VecDeque<String>,
}

impl Conn {
    /// Handle of `path` on this connection, opening it if needed.
    pub(crate) fn file(&mut self, path: &str) -> std::result::Result<FileId, pa_smb2::Error> {
        if let Some(id) = self.files.get(path) {
            return Ok(*id);
        }
        let id = self.ctx.open(path)?;
        self.files.insert(path.to_owned(), id);
        self.order.push_back(path.to_owned());
        while self.order.len() > FILES_PER_CONNECTION {
            if let Some(old) = self.order.pop_front()
                && let Some(old_id) = self.files.remove(&old)
            {
                // A failed close only leaks a server handle until logoff.
                let _ = self.ctx.close(old_id);
            }
        }
        Ok(id)
    }

    /// Forgets (and closes) the handle of `path` after an error on it.
    pub(crate) fn forget(&mut self, path: &str) {
        if let Some(id) = self.files.remove(path) {
            self.order.retain(|p| p != path);
            let _ = self.ctx.close(id);
        }
    }
}

struct State {
    idle: Vec<Conn>,
    total: usize,
}

/// Bounded pool of connections to one share.
pub(crate) struct Pool {
    lib: &'static Smb2Lib,
    config: SmbConfig,
    state: Mutex<State>,
    freed: Condvar,
}

impl Pool {
    pub(crate) fn new(lib: &'static Smb2Lib, config: SmbConfig) -> Self {
        Self {
            lib,
            config,
            state: Mutex::new(State {
                idle: Vec::new(),
                total: 0,
            }),
            freed: Condvar::new(),
        }
    }

    pub(crate) fn config(&self) -> &SmbConfig {
        &self.config
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `op` on a pooled connection. A transient failure (the
    /// connection dropped or timed out) discards that connection and runs
    /// `op` once more on a fresh one; `op` must be idempotent.
    pub(crate) fn with_conn<T>(
        &self,
        what: &str,
        mut op: impl FnMut(&mut Conn) -> std::result::Result<T, pa_smb2::Error>,
    ) -> Result<T> {
        let mut last = None;
        for _ in 0..2 {
            let mut conn = self.take()?;
            match op(&mut conn) {
                Ok(v) => {
                    self.give_back(conn);
                    return Ok(v);
                }
                Err(e) => {
                    let err = map_error(what, &e);
                    if err.kind().is_transient() {
                        self.discard(conn);
                        last = Some(err);
                        continue;
                    }
                    self.give_back(conn);
                    return Err(err);
                }
            }
        }
        Err(last.unwrap_or_else(|| VfsError::new(ErrorKind::Other, what.to_owned())))
    }

    fn take(&self) -> Result<Conn> {
        let deadline = Instant::now() + self.config.timeout;
        let mut st = self.lock();
        loop {
            if let Some(conn) = st.idle.pop() {
                return Ok(conn);
            }
            if st.total < self.config.max_connections {
                st.total += 1;
                drop(st);
                return self.connect().inspect_err(|_| {
                    self.lock().total -= 1;
                    self.freed.notify_one();
                });
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(VfsError::new(
                    ErrorKind::TimedOut,
                    format!(
                        "smb://{}/{}: all {} connections busy",
                        self.config.server, self.config.share, self.config.max_connections
                    ),
                ));
            }
            st = self
                .freed
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn give_back(&self, conn: Conn) {
        self.lock().idle.push(conn);
        self.freed.notify_one();
    }

    fn discard(&self, conn: Conn) {
        drop(conn);
        self.lock().total -= 1;
        self.freed.notify_one();
    }

    /// Opens a new connection with the configured options.
    pub(crate) fn connect(&self) -> Result<Conn> {
        let c = &self.config;
        let url = format!("smb://{}/{}", c.server, c.share);
        let fail = |e: pa_smb2::Error| map_error(&url, &e);
        let mut ctx = Context::new(self.lib).map_err(fail)?;
        ctx.set_timeout(c.timeout_secs());
        ctx.set_version(c.version);
        ctx.set_signing(c.signing);
        ctx.set_encryption(c.encryption);
        let user = match &c.credentials {
            Some(cr) => {
                ctx.set_credentials(&cr.user, &cr.password, cr.domain.as_deref())
                    .map_err(fail)?;
                cr.user.as_str()
            }
            // Anonymous logon: an empty user, not libsmb2's default of the
            // OS login name.
            None => {
                ctx.set_credentials("", "", None).map_err(fail)?;
                ""
            }
        };
        ctx.connect(&c.server, &c.share, Some(user)).map_err(fail)?;
        Ok(Conn {
            ctx,
            files: HashMap::new(),
            order: VecDeque::new(),
        })
    }
}

/// Maps a libsmb2 error to the backend-neutral kind; `what` is the path or
/// URL (never credentials).
pub(crate) fn map_error(what: &str, e: &pa_smb2::Error) -> VfsError {
    use pa_smb2::ErrorKind as S;
    let kind = match e.kind() {
        S::NotFound => ErrorKind::NotFound,
        S::PermissionDenied => ErrorKind::PermissionDenied,
        S::AuthenticationFailed => ErrorKind::AuthenticationFailed,
        S::NotADirectory => ErrorKind::NotADirectory,
        S::IsADirectory => ErrorKind::IsADirectory,
        S::InvalidPath => ErrorKind::InvalidPath,
        S::Unavailable => ErrorKind::Unavailable,
        S::TimedOut => ErrorKind::TimedOut,
        S::Unsupported | S::LibraryUnavailable => ErrorKind::Unsupported,
        _ => ErrorKind::Other,
    };
    VfsError::new(kind, format!("{what}: {e}")).with_source(e.clone())
}
