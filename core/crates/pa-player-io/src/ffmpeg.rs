// SPDX-License-Identifier: MPL-2.0
//! FFmpeg custom I/O: a [`ByteSource`] behind an `AVIOContext`.
//!
//! libavformat reads through three callbacks: `read_packet`, `seek` and,
//! optionally, the `AVIOInterruptCB` of the format context. This module
//! provides them ([`read_packet`], [`seek`], [`interrupt`]) over an
//! [`AvioSource`], and [`AvioContext`] wraps `avio_alloc_context` /
//! `avio_context_free` so the buffer and the opaque pointer are freed
//! together.
//!
//! The FFmpeg functions are taken as function pointers ([`AvioApi`]): the
//! caller resolves them from the LGPL shared libraries built by
//! `tools/ffmpeg` (or from its own FFmpeg bindings), so this crate never
//! links FFmpeg.
//!
//! ```text
//! let api = AvioApi { av_malloc, av_free, avio_alloc_context, avio_context_free };
//! let io = unsafe { AvioContext::new(api, source, 64 * 1024)? };
//! let fmt = avformat_alloc_context();
//! (*fmt).pb = io.as_ptr();                 // custom I/O
//! (*fmt).flags |= AVFMT_FLAG_CUSTOM_IO;
//! (*fmt).interrupt_callback = AVIOInterruptCB { callback: interrupt, opaque: io.opaque() };
//! avformat_open_input(&mut fmt, null(), null(), null_mut());
//! … demux …
//! avformat_close_input(&mut fmt);          // first close the format context,
//! drop(io);                                // then free the AVIOContext
//! ```

use std::ffi::{c_int, c_void};
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pa_vfs::{ByteSource, ErrorKind, VfsError};

use crate::stream::{ReadOutcome, Stream};

/// `whence` flag: return the stream size instead of seeking.
pub const AVSEEK_SIZE: c_int = 0x10000;
/// `whence` flag: seek even if it is expensive; ignored (seeks are free here).
pub const AVSEEK_FORCE: c_int = 0x20000;

const fn fferrtag(tag: &[u8; 4]) -> c_int {
    let t = tag[0] as u32 | (tag[1] as u32) << 8 | (tag[2] as u32) << 16 | (tag[3] as u32) << 24;
    -(t as c_int)
}

/// `AVERROR_EOF`: end of file.
pub const AVERROR_EOF: c_int = fferrtag(b"EOF ");
/// `AVERROR_EXIT`: the read was cancelled.
pub const AVERROR_EXIT: c_int = fferrtag(b"EXIT");
/// `AVERROR(EIO)`: the source failed (`EIO` is 5 on every supported OS).
pub const AVERROR_EIO: c_int = -5;
/// `AVERROR(EINVAL)`: bad seek arguments (`EINVAL` is 22 on every supported OS).
pub const AVERROR_EINVAL: c_int = -22;

/// `read_packet` callback type of `avio_alloc_context`.
pub type ReadPacketFn =
    unsafe extern "C" fn(opaque: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int;
/// `write_packet` callback type of `avio_alloc_context` (always null here).
pub type WritePacketFn =
    unsafe extern "C" fn(opaque: *mut c_void, buf: *const u8, buf_size: c_int) -> c_int;
/// `seek` callback type of `avio_alloc_context`.
pub type SeekFn = unsafe extern "C" fn(opaque: *mut c_void, offset: i64, whence: c_int) -> i64;
/// `AVIOInterruptCB.callback` type.
pub type InterruptFn = unsafe extern "C" fn(opaque: *mut c_void) -> c_int;

/// The FFmpeg (libavutil, libavformat) functions [`AvioContext`] needs.
///
/// All four must come from the same FFmpeg build that will use the context.
#[derive(Debug, Clone, Copy)]
pub struct AvioApi {
    /// `void *av_malloc(size_t size)`.
    pub av_malloc: unsafe extern "C" fn(size: usize) -> *mut c_void,
    /// `void av_free(void *ptr)`.
    pub av_free: unsafe extern "C" fn(ptr: *mut c_void),
    /// `AVIOContext *avio_alloc_context(…)`.
    pub avio_alloc_context: unsafe extern "C" fn(
        buffer: *mut u8,
        buffer_size: c_int,
        write_flag: c_int,
        opaque: *mut c_void,
        read_packet: Option<ReadPacketFn>,
        write_packet: Option<WritePacketFn>,
        seek: Option<SeekFn>,
    ) -> *mut c_void,
    /// `void avio_context_free(AVIOContext **s)`.
    pub avio_context_free: unsafe extern "C" fn(s: *mut *mut c_void),
}

/// The opaque state behind [`read_packet`], [`seek`] and [`interrupt`]:
/// a [`ByteSource`] with a cursor.
///
/// Keep it boxed (its address is handed to FFmpeg) and alive until the
/// `AVIOContext` and the format context that use it are freed.
#[derive(Debug)]
pub struct AvioSource {
    stream: Stream,
}

impl AvioSource {
    /// Wraps `source` with the cursor at offset 0.
    #[must_use]
    pub fn new(source: Arc<dyn ByteSource>) -> Box<Self> {
        Box::new(Self {
            stream: Stream::new(source),
        })
    }

    /// The `opaque` pointer for `avio_alloc_context` and `AVIOInterruptCB`.
    #[must_use]
    pub fn opaque(&self) -> *mut c_void {
        std::ptr::from_ref(self).cast_mut().cast()
    }

    /// Current read position.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.stream.position()
    }

    /// Makes further reads fail with [`AVERROR_EXIT`] and [`interrupt`]
    /// return 1, so a blocked `avformat_open_input` / `av_read_frame` gives
    /// up. Safe to call from any thread.
    pub fn cancel(&self) {
        self.stream.cancel();
    }

    /// The cancel flag, for a UI thread that outlives no borrow of `self`.
    #[must_use]
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.stream.cancel_flag()
    }

    /// The error behind the last [`AVERROR_EIO`], with its protocol-neutral
    /// [`ErrorKind`] (e.g. `AuthenticationFailed`, `Unavailable`).
    pub fn take_error(&self) -> Option<VfsError> {
        self.stream.take_error()
    }
}

/// `read_packet` over an [`AvioSource`].
///
/// Returns the number of bytes read, [`AVERROR_EOF`] at the end,
/// [`AVERROR_EXIT`] after [`AvioSource::cancel`] and [`AVERROR_EIO`] when
/// the source fails.
///
/// # Safety
///
/// `opaque` is [`AvioSource::opaque`] of a live `AvioSource`; `buf` is
/// valid for `buf_size` bytes of writes.
pub unsafe extern "C" fn read_packet(opaque: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int {
    // SAFETY: guaranteed by the caller (see above).
    let src = unsafe { &*opaque.cast::<AvioSource>() };
    let Ok(len) = usize::try_from(buf_size) else {
        return AVERROR_EINVAL;
    };
    if len == 0 || buf.is_null() {
        return 0;
    }
    // SAFETY: guaranteed by the caller (see above).
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };
    match src.stream.read(buf) {
        // n <= buf_size, so it fits in c_int.
        ReadOutcome::Data(n) => n as c_int,
        ReadOutcome::Eof => AVERROR_EOF,
        ReadOutcome::Cancelled => AVERROR_EXIT,
        ReadOutcome::Failed => AVERROR_EIO,
    }
}

/// `seek` over an [`AvioSource`]: `SEEK_SET`, `SEEK_CUR`, `SEEK_END` and
/// [`AVSEEK_SIZE`]; [`AVSEEK_FORCE`] is ignored. Seeking past the end is
/// allowed (the next read returns [`AVERROR_EOF`]); a negative target
/// returns [`AVERROR_EINVAL`].
///
/// # Safety
///
/// `opaque` is [`AvioSource::opaque`] of a live `AvioSource`.
pub unsafe extern "C" fn seek(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    // SAFETY: guaranteed by the caller (see above).
    let src = unsafe { &*opaque.cast::<AvioSource>() };
    let size = src.stream.size();
    let whence = whence & !AVSEEK_FORCE;
    if whence & AVSEEK_SIZE != 0 {
        return i64::try_from(size).unwrap_or(i64::from(AVERROR_EINVAL));
    }
    let base = match whence {
        0 => 0,
        1 => src.stream.position(),
        2 => size,
        _ => return i64::from(AVERROR_EINVAL),
    };
    match base
        .checked_add_signed(offset)
        .and_then(|p| i64::try_from(p).ok())
    {
        Some(target) => {
            src.stream.set_position(target as u64);
            target
        }
        None => i64::from(AVERROR_EINVAL),
    }
}

/// `AVIOInterruptCB.callback`: 1 after [`AvioSource::cancel`], else 0.
///
/// # Safety
///
/// `opaque` is [`AvioSource::opaque`] of a live `AvioSource`.
pub unsafe extern "C" fn interrupt(opaque: *mut c_void) -> c_int {
    // SAFETY: guaranteed by the caller (see above).
    let src = unsafe { &*opaque.cast::<AvioSource>() };
    c_int::from(src.stream.is_cancelled())
}

/// An `AVIOContext` reading from a [`ByteSource`]; frees itself, its
/// buffer and the [`AvioSource`] on drop.
#[derive(Debug)]
pub struct AvioContext {
    api: AvioApi,
    ctx: NonNull<c_void>,
    source: Box<AvioSource>,
}

// SAFETY: the context is only touched through `&mut self`/drop, the opaque
// `AvioSource` is `Send + Sync`, and FFmpeg contexts may move between threads.
unsafe impl Send for AvioContext {}

impl AvioContext {
    /// Allocates a read-only, seekable `AVIOContext` with a `buffer_size`
    /// byte buffer (64 KiB is a good default; network read-ahead lives in
    /// the backend, ADR-0013).
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Other`] if FFmpeg cannot allocate the buffer or the context.
    ///
    /// # Safety
    ///
    /// `api` holds the real FFmpeg functions with the documented signatures,
    /// from the FFmpeg build that will use the context.
    pub unsafe fn new(
        api: AvioApi,
        source: Arc<dyn ByteSource>,
        buffer_size: usize,
    ) -> Result<Self, VfsError> {
        let size = c_int::try_from(buffer_size.max(4096)).unwrap_or(c_int::MAX);
        let source = AvioSource::new(source);
        // SAFETY: plain allocation; `size` is positive.
        let buffer = unsafe { (api.av_malloc)(size as usize) }.cast::<u8>();
        if buffer.is_null() {
            return Err(VfsError::new(ErrorKind::Other, "av_malloc failed"));
        }
        // SAFETY: `buffer` holds `size` bytes; the callbacks match their
        // C prototypes and `opaque` lives in `source`, owned by the result.
        let ctx = unsafe {
            (api.avio_alloc_context)(
                buffer,
                size,
                0,
                source.opaque(),
                Some(read_packet),
                None,
                Some(seek),
            )
        };
        match NonNull::new(ctx) {
            Some(ctx) => Ok(Self { api, ctx, source }),
            None => {
                // SAFETY: allocated above with av_malloc and not taken over.
                unsafe { (api.av_free)(buffer.cast()) };
                Err(VfsError::new(ErrorKind::Other, "avio_alloc_context failed"))
            }
        }
    }

    /// The `AVIOContext *` to put into `AVFormatContext.pb`.
    #[must_use]
    pub fn as_ptr(&self) -> *mut c_void {
        self.ctx.as_ptr()
    }

    /// The opaque [`AvioSource`]: for `AVIOInterruptCB`, cancel and errors.
    #[must_use]
    pub fn source(&self) -> &AvioSource {
        &self.source
    }

    /// The `opaque` pointer for `AVIOInterruptCB` (same as the context's).
    #[must_use]
    pub fn opaque(&self) -> *mut c_void {
        self.source.opaque()
    }
}

impl Drop for AvioContext {
    fn drop(&mut self) {
        // `AVIOContext.buffer` is the second field, after `av_class`; FFmpeg
        // may have replaced the buffer, so free the current one, as the
        // `avio_alloc_context` docs require (`av_freep(&ctx->buffer)`).
        let buffer_field = self.ctx.as_ptr().cast::<*mut c_void>().wrapping_add(1);
        let mut ctx = self.ctx.as_ptr();
        // SAFETY: `ctx` is a live AVIOContext from `avio_alloc_context`;
        // its layout starts with two pointers (av_class, buffer).
        unsafe {
            (self.api.av_free)(*buffer_field);
            *buffer_field = std::ptr::null_mut();
            (self.api.avio_context_free)(&raw mut ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::test_util::MemSource;
    use std::sync::atomic::Ordering;

    fn read(src: &AvioSource, n: usize) -> (c_int, Vec<u8>) {
        let mut buf = vec![0u8; n];
        let r = unsafe { read_packet(src.opaque(), buf.as_mut_ptr(), n as c_int) };
        (r, buf)
    }

    #[test]
    fn error_tags_match_ffmpeg() {
        // Values of the FFmpeg macros on every platform.
        assert_eq!(AVERROR_EOF, -0x2046_4F45);
        assert_eq!(AVERROR_EXIT, -0x5449_5845);
    }

    #[test]
    fn reads_until_eof() {
        let src = AvioSource::new(MemSource::new((0..10).collect(), 6));
        let (r, buf) = read(&src, 8);
        assert_eq!(r, 6);
        assert_eq!(&buf[..6], &[0, 1, 2, 3, 4, 5]);
        assert_eq!(read(&src, 8).0, 4);
        assert_eq!(read(&src, 8).0, AVERROR_EOF);
        assert_eq!(read(&src, 0).0, 0);
        let mut b = [0u8; 1];
        assert_eq!(
            unsafe { read_packet(src.opaque(), b.as_mut_ptr(), -1) },
            AVERROR_EINVAL
        );
    }

    #[test]
    fn seeks_with_every_whence() {
        let src = AvioSource::new(MemSource::new((0..100).collect(), 64));
        let o = src.opaque();
        unsafe {
            assert_eq!(seek(o, 0, AVSEEK_SIZE), 100);
            assert_eq!(seek(o, 10, 0), 10);
            assert_eq!(seek(o, 5, 1), 15);
            assert_eq!(seek(o, -20, 2 | AVSEEK_FORCE), 80);
            assert_eq!(read(&src, 1).1, vec![80]);
            assert_eq!(seek(o, -1, 0), i64::from(AVERROR_EINVAL));
            assert_eq!(seek(o, 0, 7), i64::from(AVERROR_EINVAL));
            assert_eq!(src.position(), 81);
            assert_eq!(seek(o, 500, 0), 500);
        }
        assert_eq!(read(&src, 4).0, AVERROR_EOF);
    }

    #[test]
    fn failures_and_cancel_map_to_averror() {
        let mem = MemSource::new(vec![0; 10], 4);
        let src = AvioSource::new(mem.clone());
        mem.fail.store(true, Ordering::Relaxed);
        assert_eq!(read(&src, 4).0, AVERROR_EIO);
        assert_eq!(src.take_error().unwrap().kind(), ErrorKind::Unavailable);
        assert_eq!(unsafe { interrupt(src.opaque()) }, 0);
        src.cancel_flag().store(true, Ordering::Relaxed);
        assert_eq!(unsafe { interrupt(src.opaque()) }, 1);
        assert_eq!(read(&src, 4).0, AVERROR_EXIT);
    }
}
