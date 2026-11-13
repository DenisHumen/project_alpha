// SPDX-License-Identifier: MPL-2.0
//! libmpv custom protocol: `pa://<id>` URIs read from a [`ByteSource`].
//!
//! mpv's `stream_cb` API (`<mpv/stream_cb.h>`) lets an embedder register a
//! protocol; when mpv opens a URI with that prefix it calls our open
//! callback, which looks the id up in a [`SourceResolver`] and hands mpv
//! read/seek/size/close/cancel callbacks over the source.
//!
//! ```text
//! let registry = Arc::new(SourceRegistry::new());
//! let id = registry.insert(Arc::from(vfs.open(&path)?));
//! let protocol = MpvProtocol::new(DEFAULT_SCHEME, registry.clone());
//! // `mpv_stream_cb_add_ro` resolved from the libmpv the player loaded
//! unsafe { protocol.register(mpv_handle, mpv_stream_cb_add_ro) }?;
//! mpv_command(mpv_handle, ["loadfile", &stream_uri(DEFAULT_SCHEME, &id)]);
//! // keep `protocol` alive until mpv_terminate_destroy() returns
//! ```
//!
//! `mpv_stream_cb_add_ro` is passed in as a function pointer, so this crate
//! never links libmpv.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pa_vfs::{ByteSource, ErrorKind, VfsError};

use crate::stream::{ReadOutcome, Stream};

/// The scheme the app registers: `pa://<id>`.
pub const DEFAULT_SCHEME: &str = "pa";

/// `MPV_ERROR_NOMEM`.
pub const MPV_ERROR_NOMEM: c_int = -2;
/// `MPV_ERROR_INVALID_PARAMETER`.
pub const MPV_ERROR_INVALID_PARAMETER: c_int = -4;
/// `MPV_ERROR_LOADING_FAILED`: the URI cannot be opened.
pub const MPV_ERROR_LOADING_FAILED: c_int = -13;
/// `MPV_ERROR_UNSUPPORTED`.
pub const MPV_ERROR_UNSUPPORTED: c_int = -18;
/// `MPV_ERROR_GENERIC`.
pub const MPV_ERROR_GENERIC: c_int = -20;

/// `mpv_stream_cb_read_fn`.
pub type ReadFn = unsafe extern "C" fn(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64;
/// `mpv_stream_cb_seek_fn`.
pub type SeekFn = unsafe extern "C" fn(cookie: *mut c_void, offset: i64) -> i64;
/// `mpv_stream_cb_size_fn`.
pub type SizeFn = unsafe extern "C" fn(cookie: *mut c_void) -> i64;
/// `mpv_stream_cb_close_fn`.
pub type CloseFn = unsafe extern "C" fn(cookie: *mut c_void);
/// `mpv_stream_cb_cancel_fn` (client API 1.106+).
pub type CancelFn = unsafe extern "C" fn(cookie: *mut c_void);
/// `mpv_stream_cb_open_ro_fn`.
pub type OpenRoFn = unsafe extern "C" fn(
    user_data: *mut c_void,
    uri: *mut c_char,
    info: *mut StreamCbInfo,
) -> c_int;
/// `int mpv_stream_cb_add_ro(mpv_handle *, const char *protocol, void *user_data, mpv_stream_cb_open_ro_fn)`.
pub type StreamCbAddRoFn = unsafe extern "C" fn(
    mpv: *mut c_void,
    protocol: *const c_char,
    user_data: *mut c_void,
    open_fn: OpenRoFn,
) -> c_int;

/// `mpv_stream_cb_info`, filled by the open callback.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StreamCbInfo {
    /// Opaque per-stream pointer passed to the other callbacks.
    pub cookie: *mut c_void,
    /// Mandatory.
    pub read_fn: Option<ReadFn>,
    /// Optional; `None` means not seekable.
    pub seek_fn: Option<SeekFn>,
    /// Optional; `None` means unknown size.
    pub size_fn: Option<SizeFn>,
    /// Mandatory; releases the cookie.
    pub close_fn: Option<CloseFn>,
    /// Optional; aborts blocking reads, called from another thread.
    pub cancel_fn: Option<CancelFn>,
}

/// Turns the id of a `pa://<id>` URI into a source to play.
///
/// The source manager implements this on top of `Vfs::open`; the simple
/// [`SourceRegistry`] covers sources opened ahead of time. Called on an mpv
/// thread; may block (network open).
pub trait SourceResolver: Send + Sync + fmt::Debug {
    /// Opens the source for `id` (everything after `<scheme>://`).
    ///
    /// # Errors
    ///
    /// Any [`VfsError`]; mpv then reports the file as failed to load.
    fn resolve(&self, id: &str) -> pa_vfs::Result<Arc<dyn ByteSource>>;
}

/// Already opened sources by id.
#[derive(Debug, Default)]
pub struct SourceRegistry {
    next: AtomicU64,
    sources: Mutex<HashMap<String, Arc<dyn ByteSource>>>,
}

impl SourceRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `source` under a fresh id (`s<n>`) and returns the id.
    pub fn insert(&self, source: Arc<dyn ByteSource>) -> String {
        let id = format!("s{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        self.insert_as(id.clone(), source);
        id
    }

    /// Registers `source` under `id`, replacing an earlier one.
    pub fn insert_as(&self, id: impl Into<String>, source: Arc<dyn ByteSource>) {
        self.lock().insert(id.into(), source);
    }

    /// Unregisters `id`. Streams mpv already opened keep their source.
    pub fn remove(&self, id: &str) -> Option<Arc<dyn ByteSource>> {
        self.lock().remove(id)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<dyn ByteSource>>> {
        self.sources.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SourceResolver for SourceRegistry {
    fn resolve(&self, id: &str) -> pa_vfs::Result<Arc<dyn ByteSource>> {
        self.lock()
            .get(id)
            .cloned()
            .ok_or_else(|| VfsError::new(ErrorKind::NotFound, format!("no source with id {id:?}")))
    }
}

/// `<scheme>://<id>`, the URI to give `loadfile`.
#[must_use]
pub fn stream_uri(scheme: &str, id: &str) -> String {
    format!("{scheme}://{id}")
}

/// A protocol handler for one mpv core: scheme plus resolver.
#[derive(Debug)]
pub struct MpvProtocol {
    scheme: CString,
    prefix: String,
    resolver: Arc<dyn SourceResolver>,
    last_error: Mutex<Option<VfsError>>,
}

impl MpvProtocol {
    /// A handler for `<scheme>://<id>` URIs.
    ///
    /// # Panics
    ///
    /// If `scheme` contains a NUL byte.
    #[must_use]
    pub fn new(scheme: &str, resolver: Arc<dyn SourceResolver>) -> Box<Self> {
        Box::new(Self {
            scheme: CString::new(scheme).expect("scheme without NUL bytes"),
            prefix: format!("{scheme}://"),
            resolver,
            last_error: Mutex::new(None),
        })
    }

    /// The `user_data` pointer for [`open_ro`].
    #[must_use]
    pub fn user_data(&self) -> *mut c_void {
        std::ptr::from_ref(self).cast_mut().cast()
    }

    /// Registers the protocol on an mpv core with `mpv_stream_cb_add_ro`.
    ///
    /// # Errors
    ///
    /// The negative mpv error code (e.g. [`MPV_ERROR_INVALID_PARAMETER`] if
    /// the scheme is already registered).
    ///
    /// # Safety
    ///
    /// `mpv` is a live `mpv_handle *`, `add_ro` is `mpv_stream_cb_add_ro`
    /// of the same libmpv, and `self` stays alive (and is not moved out of
    /// its box) until `mpv_terminate_destroy()` of that core returns: mpv
    /// keeps the protocol until then and has no unregister call.
    pub unsafe fn register(&self, mpv: *mut c_void, add_ro: StreamCbAddRoFn) -> Result<(), c_int> {
        // SAFETY: guaranteed by the caller (see above).
        match unsafe { add_ro(mpv, self.scheme.as_ptr(), self.user_data(), open_ro) } {
            r if r < 0 => Err(r),
            _ => Ok(()),
        }
    }

    /// Why the last `loadfile` of this protocol failed in the open callback
    /// (unknown id, network error), with its protocol-neutral kind.
    pub fn take_error(&self) -> Option<VfsError> {
        self.last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    fn open(&self, uri: &str) -> pa_vfs::Result<Arc<dyn ByteSource>> {
        let id = uri.strip_prefix(&self.prefix).ok_or_else(|| {
            VfsError::new(
                ErrorKind::InvalidPath,
                format!("not a {} URI: {uri}", self.prefix),
            )
        })?;
        self.resolver.resolve(id)
    }
}

/// `mpv_stream_cb_open_ro_fn` for an [`MpvProtocol`].
///
/// # Safety
///
/// `user_data` is [`MpvProtocol::user_data`] of a live protocol, `uri` a
/// NUL-terminated string and `info` a writable `mpv_stream_cb_info`.
pub unsafe extern "C" fn open_ro(
    user_data: *mut c_void,
    uri: *mut c_char,
    info: *mut StreamCbInfo,
) -> c_int {
    // SAFETY: guaranteed by the caller (see above).
    let protocol = unsafe { &*user_data.cast::<MpvProtocol>() };
    // SAFETY: guaranteed by the caller (see above).
    let uri = unsafe { CStr::from_ptr(uri) }.to_string_lossy();
    let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| protocol.open(&uri)))
        .unwrap_or_else(|_| Err(VfsError::new(ErrorKind::Other, "source resolver panicked")));
    match opened {
        Ok(source) => {
            let cookie = Box::into_raw(Box::new(Stream::new(source)));
            // SAFETY: guaranteed by the caller (see above).
            unsafe {
                info.write(StreamCbInfo {
                    cookie: cookie.cast(),
                    read_fn: Some(read),
                    seek_fn: Some(seek),
                    size_fn: Some(size),
                    close_fn: Some(close),
                    cancel_fn: Some(cancel),
                });
            }
            0
        }
        Err(err) => {
            *protocol
                .last_error
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(err);
            MPV_ERROR_LOADING_FAILED
        }
    }
}

/// # Safety
///
/// `cookie` is a live cookie from [`open_ro`].
unsafe fn stream<'a>(cookie: *mut c_void) -> &'a Stream {
    // SAFETY: guaranteed by the caller.
    unsafe { &*cookie.cast::<Stream>() }
}

unsafe extern "C" fn read(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64 {
    // SAFETY: mpv passes the cookie from `open_ro` until `close`.
    let s = unsafe { stream(cookie) };
    let len = usize::try_from(nbytes)
        .unwrap_or(usize::MAX)
        .min(i64::MAX as usize);
    if len == 0 || buf.is_null() {
        return 0;
    }
    // SAFETY: mpv hands a buffer of `nbytes` writable bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), len) };
    match s.read(buf) {
        ReadOutcome::Data(n) => n as i64,
        ReadOutcome::Eof => 0,
        ReadOutcome::Cancelled | ReadOutcome::Failed => -1,
    }
}

unsafe extern "C" fn seek(cookie: *mut c_void, offset: i64) -> i64 {
    // SAFETY: mpv passes the cookie from `open_ro` until `close`.
    let s = unsafe { stream(cookie) };
    if s.is_cancelled() {
        return i64::from(MPV_ERROR_GENERIC);
    }
    match u64::try_from(offset) {
        Ok(pos) => {
            s.set_position(pos);
            offset
        }
        Err(_) => i64::from(MPV_ERROR_GENERIC),
    }
}

unsafe extern "C" fn size(cookie: *mut c_void) -> i64 {
    // SAFETY: mpv passes the cookie from `open_ro` until `close`.
    let s = unsafe { stream(cookie) };
    i64::try_from(s.size()).unwrap_or(i64::from(MPV_ERROR_UNSUPPORTED))
}

unsafe extern "C" fn close(cookie: *mut c_void) {
    // SAFETY: the cookie came from `Box::into_raw` in `open_ro`; mpv calls
    // close exactly once and no callback after it.
    drop(unsafe { Box::from_raw(cookie.cast::<Stream>()) });
}

unsafe extern "C" fn cancel(cookie: *mut c_void) {
    // SAFETY: mpv passes the cookie from `open_ro` until `close`; `Stream`
    // is `Sync`, so a call from another thread is fine.
    unsafe { stream(cookie) }.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::test_util::MemSource;
    use std::mem::MaybeUninit;

    fn open(p: &MpvProtocol, uri: &str) -> Result<StreamCbInfo, c_int> {
        let uri = CString::new(uri).unwrap();
        let mut info = MaybeUninit::<StreamCbInfo>::zeroed();
        let r = unsafe { open_ro(p.user_data(), uri.as_ptr().cast_mut(), info.as_mut_ptr()) };
        if r == 0 {
            Ok(unsafe { info.assume_init() })
        } else {
            Err(r)
        }
    }

    fn read_all(info: &StreamCbInfo, nbytes: u64) -> (i64, Vec<u8>) {
        let mut buf = vec![0u8; nbytes as usize];
        let r = unsafe { info.read_fn.unwrap()(info.cookie, buf.as_mut_ptr().cast(), nbytes) };
        (r, buf)
    }

    #[test]
    fn layout_matches_the_c_struct() {
        // Six pointers, in the order of <mpv/stream_cb.h>.
        assert_eq!(
            std::mem::size_of::<StreamCbInfo>(),
            6 * std::mem::size_of::<*const ()>()
        );
    }

    #[test]
    fn opens_reads_seeks_and_closes() {
        let registry = Arc::new(SourceRegistry::new());
        let mem = MemSource::new((0..50).collect(), 16);
        let id = registry.insert(mem.clone());
        assert_eq!(id, "s1");
        let proto = MpvProtocol::new(DEFAULT_SCHEME, registry.clone());

        let info = open(&proto, &stream_uri(DEFAULT_SCHEME, &id)).unwrap();
        assert_eq!(unsafe { info.size_fn.unwrap()(info.cookie) }, 50);
        let (r, buf) = read_all(&info, 64);
        assert_eq!(r, 16);
        assert_eq!(&buf[..16], &(0..16).collect::<Vec<u8>>()[..]);
        assert_eq!(unsafe { info.seek_fn.unwrap()(info.cookie, 45) }, 45);
        assert_eq!(read_all(&info, 64).0, 5);
        assert_eq!(read_all(&info, 64).0, 0);
        assert_eq!(
            unsafe { info.seek_fn.unwrap()(info.cookie, -1) },
            i64::from(MPV_ERROR_GENERIC)
        );
        unsafe { info.cancel_fn.unwrap()(info.cookie) };
        assert_eq!(
            unsafe { info.seek_fn.unwrap()(info.cookie, 0) },
            i64::from(MPV_ERROR_GENERIC)
        );
        assert_eq!(read_all(&info, 4).0, -1);

        // The stream owns its source: unregistering does not break it.
        registry.remove(&id).unwrap();
        assert_eq!(Arc::strong_count(&mem), 2);
        unsafe { info.close_fn.unwrap()(info.cookie) };
        assert_eq!(Arc::strong_count(&mem), 1);
    }

    #[test]
    fn source_errors_become_minus_one() {
        let registry = Arc::new(SourceRegistry::new());
        let mem = MemSource::new(vec![0; 8], 8);
        registry.insert_as("x", mem.clone());
        let proto = MpvProtocol::new("pa", registry);
        let info = open(&proto, "pa://x").unwrap();
        mem.fail.store(true, Ordering::Relaxed);
        assert_eq!(read_all(&info, 8).0, -1);
        unsafe { info.close_fn.unwrap()(info.cookie) };
    }

    #[test]
    fn unknown_ids_and_foreign_uris_fail_to_load() {
        let proto = MpvProtocol::new("pa", Arc::new(SourceRegistry::new()));
        assert_eq!(
            open(&proto, "pa://nope").unwrap_err(),
            MPV_ERROR_LOADING_FAILED
        );
        assert_eq!(proto.take_error().unwrap().kind(), ErrorKind::NotFound);
        assert_eq!(
            open(&proto, "smb://x").unwrap_err(),
            MPV_ERROR_LOADING_FAILED
        );
        assert_eq!(proto.take_error().unwrap().kind(), ErrorKind::InvalidPath);
    }

    #[test]
    fn register_passes_scheme_and_user_data() {
        unsafe extern "C" fn fake_add_ro(
            mpv: *mut c_void,
            protocol: *const c_char,
            user_data: *mut c_void,
            _open: OpenRoFn,
        ) -> c_int {
            assert_eq!(mpv as usize, 0x1234);
            assert_eq!(unsafe { CStr::from_ptr(protocol) }.to_str().unwrap(), "pa");
            assert!(!user_data.is_null());
            if unsafe { &*user_data.cast::<MpvProtocol>() }.prefix == "pa://" {
                0
            } else {
                MPV_ERROR_INVALID_PARAMETER
            }
        }
        let proto = MpvProtocol::new("pa", Arc::new(SourceRegistry::new()));
        assert_eq!(
            unsafe { proto.register(0x1234 as *mut c_void, fake_add_ro) },
            Ok(())
        );
    }
}
