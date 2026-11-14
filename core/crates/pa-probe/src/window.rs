// SPDX-License-Identifier: MPL-2.0
//! A [`ByteSource`] that only serves the head and the tail of a file.
//!
//! libavformat reads as much as it likes; this wrapper keeps a probe within
//! a fixed budget on slow network sources. Reads inside the first
//! `head` bytes or the last `tail` bytes go to the inner source; reads
//! elsewhere return end of file, which libavformat treats as a short file
//! and stops analysing. The tail serves the indexes some containers keep
//! at the end (MP4 `moov` after `mdat`) and the last timestamps that give
//! MPEG-TS its duration.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use pa_vfs::{ByteSource, ErrorKind, Result, VfsError};

/// Message of the error returned after cancellation.
pub(crate) const CANCELLED: &str = "probe cancelled";

/// See the module docs.
#[derive(Debug)]
pub(crate) struct WindowedSource {
    inner: Arc<dyn ByteSource>,
    size: u64,
    head_end: u64,
    tail_start: u64,
    cancel: Option<Arc<AtomicBool>>,
    /// Bytes delivered by the inner source.
    pub(crate) bytes_read: AtomicU64,
    /// Set when a read was cut by the window.
    pub(crate) truncated: AtomicBool,
}

impl WindowedSource {
    pub(crate) fn new(
        inner: Arc<dyn ByteSource>,
        head: u64,
        tail: u64,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Self {
        let size = inner.size();
        let head_end = head.min(size);
        let tail_start = size.saturating_sub(tail).max(head_end);
        Self {
            inner,
            size,
            head_end,
            tail_start,
            cancel,
            bytes_read: AtomicU64::new(0),
            truncated: AtomicBool::new(false),
        }
    }

    /// The readable range that contains `offset` ends here; `None` if
    /// `offset` is in the gap between head and tail.
    fn range_end(&self, offset: u64) -> Option<u64> {
        if offset < self.head_end {
            Some(self.head_end)
        } else if offset >= self.tail_start {
            Some(self.size)
        } else {
            None
        }
    }
}

impl ByteSource for WindowedSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
        {
            return Err(VfsError::new(ErrorKind::Other, CANCELLED));
        }
        if offset >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let Some(end) = self.range_end(offset) else {
            self.truncated.store(true, Ordering::Relaxed);
            return Ok(0);
        };
        let want = usize::try_from(end - offset).map_or(buf.len(), |n| n.min(buf.len()));
        // Cut at the end of the head while a gap follows it.
        if want < buf.len() && end < self.tail_start {
            self.truncated.store(true, Ordering::Relaxed);
        }
        let n = self.inner.read_at(offset, &mut buf[..want])?;
        self.bytes_read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Mem(Vec<u8>);

    impl ByteSource for Mem {
        fn size(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
            let data = self.0.get(offset as usize..).unwrap_or(&[]);
            let n = data.len().min(buf.len());
            buf[..n].copy_from_slice(&data[..n]);
            Ok(n)
        }
    }

    fn src(len: usize, head: u64, tail: u64) -> WindowedSource {
        WindowedSource::new(
            Arc::new(Mem((0..len).map(|i| i as u8).collect())),
            head,
            tail,
            None,
        )
    }

    #[test]
    fn serves_head_and_tail_only() {
        let w = src(100, 10, 20);
        let mut buf = [0u8; 30];
        assert_eq!(w.read_at(0, &mut buf).unwrap(), 10);
        assert_eq!(buf[9], 9);
        assert!(w.truncated.load(Ordering::Relaxed));
        assert_eq!(w.read_at(50, &mut buf).unwrap(), 0);
        assert_eq!(w.read_at(80, &mut buf).unwrap(), 20);
        assert_eq!(buf[0], 80);
        assert_eq!(w.read_at(100, &mut buf).unwrap(), 0);
        assert_eq!(w.bytes_read.load(Ordering::Relaxed), 30);
    }

    #[test]
    fn small_files_are_read_whole() {
        let w = src(50, 40, 20);
        let mut buf = [0u8; 64];
        assert_eq!(w.read_at(0, &mut buf).unwrap(), 40);
        assert_eq!(w.read_at(40, &mut buf).unwrap(), 10);
        assert!(!w.truncated.load(Ordering::Relaxed));
    }

    #[test]
    fn cancel_fails_reads() {
        let flag = Arc::new(AtomicBool::new(false));
        let w = WindowedSource::new(Arc::new(Mem(vec![0; 10])), 8, 0, Some(flag.clone()));
        let mut buf = [0u8; 4];
        assert_eq!(w.read_at(0, &mut buf).unwrap(), 4);
        flag.store(true, Ordering::Relaxed);
        assert_eq!(w.read_at(0, &mut buf).unwrap_err().kind(), ErrorKind::Other);
    }
}
