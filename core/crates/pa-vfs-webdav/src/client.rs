// SPDX-License-Identifier: MPL-2.0
//! [`WebDavFs`]: the [`Vfs`] implementation and its HTTP plumbing.

use std::fmt;
use std::io::Read;
use std::sync::{Arc, Mutex, PoisonError};

use pa_vfs::{ByteSource, DirEntry, EntryKind, ErrorKind, Metadata, Result, Vfs, VfsError, VfsPath};
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, RANGE, WWW_AUTHENTICATE};
use reqwest::{Method, StatusCode, Url};

use crate::auth::{AuthState, parse_challenges};
use crate::config::{Credentials, WebDavConfig};
use crate::href::Base;
use crate::propfind::{self, DavEntry};
use crate::source::WebDavSource;
use crate::tls::{self, TlsTrust, UntrustedCertificate};

/// Redirect hops followed per request (as browsers and reqwest do).
const MAX_REDIRECTS: usize = 10;
/// Upper bound for a PROPFIND reply (a directory of ~100k entries).
const MAX_PROPFIND_BYTES: u64 = 64 * 1024 * 1024;

/// A WebDAV collection as a [`Vfs`] (NET-004).
///
/// - Listing and metadata: `PROPFIND` with `Depth: 1` / `Depth: 0`, only
///   `resourcetype`, `getcontentlength` and `getlastmodified` requested.
/// - Reads: ranged `GET` with a per-file read-ahead cache
///   ([`WebDavConfig::read_ahead`]); servers that ignore `Range` still work
///   (the prefix is skipped), just slower.
/// - Authentication: Basic and Digest (MD5, SHA-256, `-sess`, `qop=auth`),
///   chosen from the server's challenge; credentials are sent only to the
///   origin of the source URL.
/// - Redirects: up to 10 hops, method preserved (a PROPFIND stays a
///   PROPFIND, unlike in browsers).
/// - TLS: rustls; platform trust or a pinned self-signed certificate
///   ([`TlsTrust`]).
///
/// Blocking (ADR-0013): call it from worker threads, not from a tokio
/// runtime thread (use `spawn_blocking`). Cheap to clone the `Arc` around it;
/// connections are pooled and shared by all opened files.
#[derive(Clone)]
pub struct WebDavFs {
    inner: Arc<Inner>,
}

pub(crate) struct Inner {
    base: Base,
    http: Client,
    creds: Option<Credentials>,
    auth: Mutex<Option<AuthState>>,
    pub(crate) read_ahead: usize,
    pub(crate) cache_blocks: usize,
}

impl fmt::Debug for WebDavFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebDavFs")
            .field("url", &self.inner.base.url().as_str())
            .field("user", &self.inner.creds.as_ref().map(|c| c.username.as_str()))
            .finish_non_exhaustive()
    }
}

/// A request to send through [`Inner::send`].
pub(crate) struct Req<'a> {
    pub method: Method,
    pub url: Url,
    pub depth: Option<&'static str>,
    pub range: Option<(u64, u64)>,
    pub body: Option<&'a str>,
}

impl Req<'_> {
    pub(crate) fn new(method: Method, url: Url) -> Self {
        Self { method, url, depth: None, range: None, body: None }
    }
}

impl WebDavFs {
    /// Builds the client. Does not touch the network: a wrong address or
    /// password shows up on the first call.
    pub fn new(config: WebDavConfig) -> Result<Self> {
        let base = Base::parse(&config.url)?;
        let builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(config.connect_timeout)
            .timeout(config.timeout)
            .user_agent(concat!("Kinolune/", env!("CARGO_PKG_VERSION")));
        let builder = match config.tls {
            TlsTrust::System => {
                // Fails only if a provider is already installed: that is fine.
                let _ = rustls::crypto::ring::default_provider().install_default();
                builder
            }
            TlsTrust::Pinned(fp) => builder.tls_backend_preconfigured(tls::pinned_config(fp)?),
        };
        let http = builder
            .build()
            .map_err(|e| VfsError::new(ErrorKind::Other, format!("HTTP client: {e}")).with_source(e))?;
        Ok(Self {
            inner: Arc::new(Inner {
                base,
                http,
                creds: config.credentials,
                auth: Mutex::new(None),
                read_ahead: config.read_ahead,
                cache_blocks: config.cache_blocks.max(1),
            }),
        })
    }

    /// The normalized root URL (ends with `/`).
    #[must_use]
    pub fn url(&self) -> &str {
        self.inner.base.url().as_str()
    }

    /// PROPFIND `path`; returns every entry and the one describing `path`.
    fn propfind(&self, path: &VfsPath, depth: &'static str) -> Result<(Vec<(Vec<String>, DavEntry)>, usize)> {
        let url = self.inner.base.url_of(path, false);
        let mut req = Req::new(Method::from_bytes(b"PROPFIND").expect("valid method"), url);
        req.depth = Some(depth);
        req.body = Some(propfind::BODY);
        let resp = self.inner.send(&req, path)?;
        let status = resp.status();
        if status != StatusCode::MULTI_STATUS && status != StatusCode::OK {
            return Err(status_error(status, path));
        }
        let root = self.inner.base.as_seen_by(resp.url(), path);
        let mut body = Vec::new();
        resp.take(MAX_PROPFIND_BYTES)
            .read_to_end(&mut body)
            .map_err(|e| VfsError::from_io(e, path))?;
        let entries: Vec<_> = propfind::parse(&body)?
            .into_iter()
            .filter_map(|e| Some((root.relative(&e.href)?, e)))
            .collect();
        let want: Vec<&str> = path.components().collect();
        let me = entries
            .iter()
            .position(|(rel, _)| *rel == want)
            .or_else(|| (!entries.is_empty()).then_some(0))
            .ok_or_else(|| VfsError::new(ErrorKind::Other, format!("{path}: empty PROPFIND reply")))?;
        if entries[me].1.failed {
            return Err(VfsError::new(ErrorKind::NotFound, path.to_string()));
        }
        Ok((entries, me))
    }

    fn size_from_head(&self, path: &VfsPath, url: &Url) -> Result<u64> {
        let resp = self.inner.send(&Req::new(Method::HEAD, url.clone()), path)?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status(), path));
        }
        resp.headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()?.parse().ok())
            .ok_or_else(|| VfsError::new(ErrorKind::Unsupported, format!("{path}: server reports no file size")))
    }

    /// Metadata of an entry; a file without `getcontentlength` (rare, some
    /// embedded servers) costs one extra HEAD request.
    fn metadata(&self, path: &VfsPath, e: &DavEntry) -> Result<Metadata> {
        if e.is_collection {
            return Ok(Metadata { kind: EntryKind::Dir, size: 0, modified_ms: e.modified_ms });
        }
        let size = match e.size {
            Some(s) => s,
            None => self.size_from_head(path, &self.inner.base.url_of(path, false))?,
        };
        Ok(Metadata { kind: EntryKind::File, size, modified_ms: e.modified_ms })
    }
}

impl Vfs for WebDavFs {
    fn list(&self, dir: &VfsPath) -> Result<Vec<DirEntry>> {
        let (entries, me) = self.propfind(dir, "1")?;
        if !entries[me].1.is_collection {
            return Err(VfsError::new(ErrorKind::NotADirectory, dir.to_string()));
        }
        let depth = dir.components().count();
        let mut out: Vec<DirEntry> = Vec::with_capacity(entries.len().saturating_sub(1));
        for (i, (rel, e)) in entries.iter().enumerate() {
            if i == me || e.failed || rel.len() != depth + 1 || !rel.iter().zip(dir.components()).all(|(a, b)| a == b) {
                continue;
            }
            let name = &rel[depth];
            // `..`, empty names and decoded `/` cannot be a VfsPath component.
            let Ok(path) = dir.join(name) else { continue };
            if out.iter().any(|d| d.name == *name) {
                continue;
            }
            let metadata = self.metadata(&path, e)?;
            out.push(DirEntry { name: name.clone(), path, metadata });
        }
        Ok(out)
    }

    fn stat(&self, path: &VfsPath) -> Result<Metadata> {
        let (entries, me) = self.propfind(path, "0")?;
        self.metadata(path, &entries[me].1)
    }

    fn open(&self, path: &VfsPath) -> Result<Box<dyn ByteSource>> {
        let (entries, me) = self.propfind(path, "0")?;
        let e = &entries[me].1;
        if e.is_collection {
            return Err(VfsError::new(ErrorKind::IsADirectory, path.to_string()));
        }
        let size = self.metadata(path, e)?.size;
        let url = self.inner.base.url_of(path, false);
        Ok(Box::new(WebDavSource::new(Arc::clone(&self.inner), url, path.to_string(), size)))
    }
}

impl Inner {
    /// Sends `req`, answering auth challenges and following redirects.
    /// `what` names the resource in errors (never the credentials).
    pub(crate) fn send(&self, req: &Req<'_>, what: &dyn fmt::Display) -> Result<Response> {
        let mut url = req.url.clone();
        let mut redirects = 0;
        let mut auth_retries = 0;
        loop {
            let mut rb = self.http.request(req.method.clone(), url.clone());
            if let Some(d) = req.depth {
                rb = rb.header("Depth", d);
            }
            if let Some((a, b)) = req.range {
                rb = rb.header(RANGE, format!("bytes={a}-{b}"));
            }
            if let Some(body) = req.body {
                rb = rb
                    .header(CONTENT_TYPE, "application/xml; charset=utf-8")
                    .body(body.to_owned());
            }
            let same_origin = self.base.same_origin(&url);
            let creds = self.creds.as_ref().filter(|_| same_origin);
            let mut sent_auth = false;
            if let Some(c) = creds {
                let mut guard = self.auth.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(state) = guard.as_mut() {
                    let uri = match url.query() {
                        Some(q) => format!("{}?{q}", url.path()),
                        None => url.path().to_owned(),
                    };
                    rb = rb.header(AUTHORIZATION, state.header(c, req.method.as_str(), &uri));
                    sent_auth = true;
                }
            }
            let resp = rb.send().map_err(|e| request_error(&e, what))?;
            let status = resp.status();

            if status == StatusCode::UNAUTHORIZED {
                let Some(c) = creds else {
                    return Err(VfsError::new(
                        ErrorKind::AuthenticationFailed,
                        format!("{what}: the server requires a user name and password"),
                    ));
                };
                let challenges = parse_challenges(
                    resp.headers()
                        .get_all(WWW_AUTHENTICATE)
                        .iter()
                        .filter_map(|v| v.to_str().ok()),
                );
                let stale = AuthState::is_stale(&challenges);
                if auth_retries >= 2 || (sent_auth && auth_retries >= 1 && !stale) {
                    return Err(VfsError::new(
                        ErrorKind::AuthenticationFailed,
                        format!("{what}: the server rejected the user name or password"),
                    ));
                }
                let state = AuthState::from_challenges(&challenges, c).ok_or_else(|| {
                    let schemes: Vec<_> = challenges.iter().map(|c| c.scheme.as_str()).collect();
                    VfsError::new(
                        ErrorKind::AuthenticationFailed,
                        format!("{what}: unsupported authentication {schemes:?} (Basic and Digest are supported)"),
                    )
                })?;
                *self.auth.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
                auth_retries += 1;
                continue;
            }

            if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
                let next = resp
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|l| url.join(l).ok())
                    .ok_or_else(|| VfsError::new(ErrorKind::Other, format!("{what}: redirect without a valid Location")))?;
                redirects += 1;
                if redirects > MAX_REDIRECTS || !matches!(next.scheme(), "http" | "https") {
                    return Err(VfsError::new(ErrorKind::Other, format!("{what}: too many or bad redirects")));
                }
                url = next;
                continue;
            }
            return Ok(resp);
        }
    }
}

/// Maps a non-success HTTP status to a [`VfsError`].
pub(crate) fn status_error(status: StatusCode, what: &dyn fmt::Display) -> VfsError {
    let kind = match status.as_u16() {
        401 => ErrorKind::AuthenticationFailed,
        403 => ErrorKind::PermissionDenied,
        // 409 Conflict: a missing intermediate collection (RFC 4918 §9.1).
        404 | 409 | 410 => ErrorKind::NotFound,
        405 | 501 => ErrorKind::Unsupported,
        408 | 504 => ErrorKind::TimedOut,
        429 | 502 | 503 => ErrorKind::Unavailable,
        _ => ErrorKind::Other,
    };
    VfsError::new(kind, format!("{what}: HTTP {status}"))
}

/// Maps a transport error; certificate failures get an
/// [`UntrustedCertificate`] source.
pub(crate) fn request_error(e: &reqwest::Error, what: &dyn fmt::Display) -> VfsError {
    if let Some(msg) = tls::certificate_error(e) {
        return VfsError::new(ErrorKind::Other, format!("{what}: untrusted TLS certificate ({msg})"))
            .with_source(UntrustedCertificate(msg));
    }
    let kind = if e.is_timeout() {
        ErrorKind::TimedOut
    } else if e.is_connect() {
        ErrorKind::Unavailable
    } else {
        ErrorKind::Other
    };
    let mut msg = format!("{what}: {e}");
    let mut cur = std::error::Error::source(e);
    while let Some(s) = cur {
        msg.push_str(&format!(": {s}"));
        cur = s.source();
    }
    VfsError::new(kind, msg)
}
