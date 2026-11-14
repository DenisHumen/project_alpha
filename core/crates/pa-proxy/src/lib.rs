// SPDX-License-Identifier: MPL-2.0
//! Loopback HTTP Range server for media sources (NET-007).
//!
//! Players that only take a URL (media_kit/libmpv in the Flutter app,
//! AVPlayer, ExoPlayer) read files of network sources through this proxy:
//! it listens on `127.0.0.1:<random port>` and serves any
//! [`ByteSource`](pa_vfs::ByteSource) an [`Opener`] returns, with HTTP
//! `Range` support, so seeking costs one positional read on the source.
//!
//! - Every URL starts with a random token (`/<token>/…`, 122 random bits);
//!   a request without it gets `403`. The port is open to every local
//!   process, the token is what keeps other apps and web pages out.
//! - Only `GET` and `HEAD`; one `bytes=` range per request (`a-b`, `a-`,
//!   `-n`); `206` with `Content-Range`, `416` outside the file.
//! - HTTP/1.1 keep-alive; at most [`ProxyOptions::max_connections`]
//!   connections at once (more get `503`); idle connections close after
//!   [`ProxyOptions::idle_timeout`].
//! - Blocking, one thread per connection (ADR-0013: the VFS is blocking).
//!
//! ```
//! use std::io::{Read, Write};
//! use std::sync::Arc;
//! use pa_proxy::{Opener, Proxy, ProxyOptions};
//! use pa_vfs::{ByteSource, ErrorKind, VfsError};
//!
//! #[derive(Debug)]
//! struct Bytes(Vec<u8>);
//! impl ByteSource for Bytes {
//!     fn size(&self) -> u64 { self.0.len() as u64 }
//!     fn read_at(&self, offset: u64, buf: &mut [u8]) -> pa_vfs::Result<usize> {
//!         let start = (offset as usize).min(self.0.len());
//!         let n = buf.len().min(self.0.len() - start);
//!         buf[..n].copy_from_slice(&self.0[start..start + n]);
//!         Ok(n)
//!     }
//! }
//!
//! struct One;
//! impl Opener for One {
//!     fn open(&self, resource: &str) -> pa_vfs::Result<Arc<dyn ByteSource>> {
//!         match resource {
//!             "movie.mkv" => Ok(Arc::new(Bytes(b"0123456789".to_vec()))),
//!             _ => Err(VfsError::new(ErrorKind::NotFound, resource)),
//!         }
//!     }
//! }
//!
//! let proxy = Proxy::start(Arc::new(One), ProxyOptions::default())?;
//! let url = proxy.url("movie.mkv");
//! assert!(url.starts_with("http://127.0.0.1:"));
//!
//! let path = &url[url.find('/').unwrap() + 2..];
//! let path = &path[path.find('/').unwrap()..];
//! let mut s = std::net::TcpStream::connect(proxy.local_addr())?;
//! write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\nRange: bytes=2-4\r\nConnection: close\r\n\r\n")?;
//! let mut reply = String::new();
//! s.read_to_string(&mut reply)?;
//! assert!(reply.starts_with("HTTP/1.1 206"));
//! assert!(reply.ends_with("\r\n\r\n234"));
//! # Ok::<(), std::io::Error>(())
//! ```

mod http;

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use pa_vfs::ByteSource;

/// Opens what a proxy URL names.
///
/// `resource` is the part of the URL path after the token, percent-decoded
/// (UTF-8, `/`-separated, never empty). Its meaning is the opener's: the
/// library uses `<source id>/<path inside the source>`. Called on the
/// connection's thread for every request, so an opener that talks to the
/// network should cache open sources.
pub trait Opener: Send + Sync {
    /// The file for `resource`. [`ErrorKind::NotFound`](pa_vfs::ErrorKind)
    /// becomes `404`, permission and authentication errors `403`, an
    /// unreachable source `502`, a timeout `504`.
    fn open(&self, resource: &str) -> pa_vfs::Result<Arc<dyn ByteSource>>;
}

/// Limits of a [`Proxy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyOptions {
    /// Connections served at once; more get `503` (default 16).
    pub max_connections: usize,
    /// An idle keep-alive connection, or a client that stops reading, is
    /// closed after this long (default 60 s).
    pub idle_timeout: Duration,
    /// Bytes read from the source per write to the socket (default 256 KiB).
    pub chunk_size: usize,
}

impl Default for ProxyOptions {
    fn default() -> Self {
        Self {
            max_connections: 16,
            idle_timeout: Duration::from_secs(60),
            chunk_size: 256 * 1024,
        }
    }
}

/// State shared by the accept loop and the connection threads.
pub(crate) struct Shared {
    pub(crate) opener: Arc<dyn Opener>,
    pub(crate) options: ProxyOptions,
    pub(crate) token: String,
    pub(crate) stop: AtomicBool,
    pub(crate) connections: AtomicUsize,
}

/// A running loopback server; stops when dropped.
pub struct Proxy {
    shared: Arc<Shared>,
    addr: SocketAddr,
    accept: Option<JoinHandle<()>>,
}

impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The token is a secret: not printed.
        f.debug_struct("Proxy")
            .field("addr", &self.addr)
            .field("options", &self.shared.options)
            .finish_non_exhaustive()
    }
}

impl Proxy {
    /// Binds `127.0.0.1` on a free port and starts serving.
    pub fn start(opener: Arc<dyn Opener>, options: ProxyOptions) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            opener,
            options: ProxyOptions {
                max_connections: options.max_connections.max(1),
                chunk_size: options.chunk_size.max(4096),
                ..options
            },
            token: uuid::Uuid::new_v4().simple().to_string(),
            stop: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
        });
        let accept_shared = Arc::clone(&shared);
        let accept = std::thread::Builder::new()
            .name("pa-proxy".into())
            .spawn(move || accept_loop(&listener, &accept_shared))?;
        Ok(Self {
            shared,
            addr,
            accept: Some(accept),
        })
    }

    /// The listening address (`127.0.0.1:<port>`).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>/<token>`: append `/` and a percent-encoded
    /// resource ([`encode_path`]) to get a file URL.
    #[must_use]
    pub fn base_url(&self) -> String {
        format!("http://{}/{}", self.addr, self.shared.token)
    }

    /// The URL of `resource` (encoded with [`encode_path`]).
    #[must_use]
    pub fn url(&self, resource: &str) -> String {
        format!(
            "{}/{}",
            self.base_url(),
            encode_path(resource.trim_start_matches('/'))
        )
    }

    /// Connections being served now.
    #[must_use]
    pub fn connections(&self) -> usize {
        self.shared.connections.load(Ordering::Relaxed)
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Wake the blocking accept.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
    }
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let busy =
            shared.connections.fetch_add(1, Ordering::AcqRel) >= shared.options.max_connections;
        let conn_shared = Arc::clone(shared);
        let spawned = std::thread::Builder::new()
            .name("pa-proxy-conn".into())
            .spawn(move || {
                if busy {
                    http::reject_busy(stream, &conn_shared);
                } else {
                    http::serve(stream, &conn_shared);
                }
                conn_shared.connections.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            // The closure never ran (and dropped the stream): undo the count.
            shared.connections.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// Percent-encodes a `/`-separated path for a URL: every byte except
/// unreserved characters (`A-Z a-z 0-9 - . _ ~`) and `/`.
#[must_use]
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes `%XX` escapes; `None` for a broken escape or non-UTF-8 result.
#[must_use]
pub fn decode_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip() {
        for p in [
            "Movies/Dune (2021)/Dune.mkv",
            "Сериалы/Ёлки #1?.mkv",
            "a%b/c+d e",
        ] {
            let e = encode_path(p);
            assert!(!e.contains(' ') && !e.contains('?') && !e.contains('#'));
            assert_eq!(decode_path(&e).as_deref(), Some(p));
        }
        assert_eq!(decode_path("%zz"), None);
        assert_eq!(decode_path("%4"), None);
        assert_eq!(decode_path("%FF"), None);
    }
}
