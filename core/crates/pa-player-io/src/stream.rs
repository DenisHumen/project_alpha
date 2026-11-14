// SPDX-License-Identifier: MPL-2.0
//! Cursor shared by the FFmpeg and mpv adapters.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pa_vfs::{ByteSource, ErrorKind, VfsError};

/// What a read through the cursor produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadOutcome {
    /// `n > 0` bytes were copied.
    Data(usize),
    /// The cursor is at or past the end of the file.
    Eof,
    /// The read was cancelled ([`Stream::cancel`]).
    Cancelled,
    /// The source failed; the error is kept in [`Stream::take_error`].
    Failed,
}

/// A [`ByteSource`] with a cursor, a cancel flag and the last error.
///
/// Players call the read and seek callbacks of one stream from one thread at
/// a time, but mpv may call `cancel_fn` from any thread, so every field is
/// shared-mutable through atomics or a mutex and the callbacks only ever
/// take `&Stream`.
#[derive(Debug)]
pub(crate) struct Stream {
    source: Arc<dyn ByteSource>,
    pos: AtomicU64,
    cancelled: Arc<AtomicBool>,
    last_error: Mutex<Option<VfsError>>,
}

impl Stream {
    pub(crate) fn new(source: Arc<dyn ByteSource>) -> Self {
        Self {
            source,
            pos: AtomicU64::new(0),
            cancelled: Arc::new(AtomicBool::new(false)),
            last_error: Mutex::new(None),
        }
    }

    pub(crate) fn size(&self) -> u64 {
        self.source.size()
    }

    pub(crate) fn position(&self) -> u64 {
        self.pos.load(Ordering::Relaxed)
    }

    pub(crate) fn set_position(&self, pos: u64) {
        self.pos.store(pos, Ordering::Relaxed);
    }

    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Reads at the cursor and advances it. Never unwinds: a panicking
    /// source is reported as an error, because unwinding into C aborts.
    pub(crate) fn read(&self, buf: &mut [u8]) -> ReadOutcome {
        if self.is_cancelled() {
            return ReadOutcome::Cancelled;
        }
        let pos = self.position();
        if buf.is_empty() || pos >= self.size() {
            return ReadOutcome::Eof;
        }
        let result = catch_unwind(AssertUnwindSafe(|| self.source.read_at(pos, buf)))
            .unwrap_or_else(|_| Err(VfsError::new(ErrorKind::Other, "byte source panicked")));
        match result {
            Ok(0) => ReadOutcome::Eof,
            Ok(n) => {
                let n = n.min(buf.len());
                self.set_position(pos + n as u64);
                ReadOutcome::Data(n)
            }
            Err(err) => {
                *self.lock_error() = Some(err);
                ReadOutcome::Failed
            }
        }
    }

    pub(crate) fn take_error(&self) -> Option<VfsError> {
        self.lock_error().take()
    }

    fn lock_error(&self) -> std::sync::MutexGuard<'_, Option<VfsError>> {
        self.last_error.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// In-memory source returning at most `chunk` bytes per call, like a
    /// network backend that hands out whatever arrived; can be told to fail.
    #[derive(Debug)]
    pub(crate) struct MemSource {
        pub data: Vec<u8>,
        pub chunk: usize,
        pub fail: AtomicBool,
        pub reads: AtomicUsize,
    }

    impl MemSource {
        pub(crate) fn new(data: Vec<u8>, chunk: usize) -> Arc<Self> {
            Arc::new(Self {
                data,
                chunk,
                fail: AtomicBool::new(false),
                reads: AtomicUsize::new(0),
            })
        }
    }

    impl ByteSource for MemSource {
        fn size(&self) -> u64 {
            self.data.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> pa_vfs::Result<usize> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            if self.fail.load(Ordering::Relaxed) {
                return Err(VfsError::new(ErrorKind::Unavailable, "share went away"));
            }
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(self.data.len());
            let n = buf.len().min(self.chunk).min(self.data.len() - start);
            buf[..n].copy_from_slice(&self.data[start..start + n]);
            Ok(n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::MemSource;
    use super::*;

    #[test]
    fn reads_advance_and_stop_at_eof() {
        let s = Stream::new(MemSource::new((0..10).collect(), 4));
        let mut buf = [0u8; 8];
        assert_eq!(s.read(&mut buf), ReadOutcome::Data(4));
        assert_eq!(&buf[..4], &[0, 1, 2, 3]);
        assert_eq!(s.read(&mut buf), ReadOutcome::Data(4));
        assert_eq!(s.read(&mut buf), ReadOutcome::Data(2));
        assert_eq!(s.read(&mut buf), ReadOutcome::Eof);
        s.set_position(100);
        assert_eq!(s.read(&mut buf), ReadOutcome::Eof);
    }

    #[test]
    fn errors_and_cancel_are_reported() {
        let src = MemSource::new(vec![1; 10], 4);
        let s = Stream::new(src.clone());
        src.fail.store(true, Ordering::Relaxed);
        let mut buf = [0u8; 4];
        assert_eq!(s.read(&mut buf), ReadOutcome::Failed);
        assert_eq!(s.take_error().unwrap().kind(), ErrorKind::Unavailable);
        assert!(s.take_error().is_none());
        s.cancel();
        assert_eq!(s.read(&mut buf), ReadOutcome::Cancelled);
    }

    #[derive(Debug)]
    struct Panicky;
    impl ByteSource for Panicky {
        fn size(&self) -> u64 {
            1
        }
        fn read_at(&self, _: u64, _: &mut [u8]) -> pa_vfs::Result<usize> {
            panic!("boom")
        }
    }

    #[test]
    fn panicking_source_becomes_an_error() {
        let s = Stream::new(Arc::new(Panicky));
        let mut buf = [0u8; 1];
        assert_eq!(s.read(&mut buf), ReadOutcome::Failed);
        assert_eq!(s.take_error().unwrap().kind(), ErrorKind::Other);
    }
}
