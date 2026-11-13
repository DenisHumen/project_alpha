// SPDX-License-Identifier: MPL-2.0
//! Random-access byte sources handed to players, the prober and the proxy.

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;

use crate::error::{ErrorKind, Result, VfsError};

/// An opened file: a fixed size and positional reads.
///
/// This is the single contract between sources and consumers (ADR-0013):
/// FFmpeg `AVIOContext` and mpv `stream_cb` adapters (NET-008), the
/// loopback HTTP Range proxy (NET-007), Media3 `DataSource`, the prober and
/// fingerprinting (LIB-004).
///
/// Rules every implementation follows (checked by [`contract`](crate::contract)):
///
/// - [`size`](Self::size) is the file size at open time and does not change.
/// - [`read_at`](Self::read_at) is positional and has no cursor, so one
///   source can serve several readers at once (`&self`, `Send + Sync`).
/// - `read_at` returns the number of bytes read. It may return fewer than
///   `buf.len()` (a short read) and returns `0` only when `offset >= size`
///   or `buf` is empty.
/// - Reads are blocking. Network backends block the calling thread on their
///   own runtime; async callers wrap calls in `spawn_blocking`.
pub trait ByteSource: Send + Sync + fmt::Debug {
    /// Size of the file in bytes, fixed at open time.
    fn size(&self) -> u64;

    /// Reads up to `buf.len()` bytes starting at `offset`.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize>;

    /// Fills `buf` completely from `offset`, retrying short reads.
    ///
    /// Fails with [`ErrorKind::Other`] ("unexpected end of file") if the
    /// file ends before `buf` is full.
    fn read_exact_at(&self, mut offset: u64, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = self.read_at(offset, buf)?;
            if n == 0 {
                return Err(VfsError::new(
                    ErrorKind::Other,
                    format!("unexpected end of file at offset {offset}"),
                ));
            }
            offset += n as u64;
            buf = &mut buf[n..];
        }
        Ok(())
    }
}

impl<T: ByteSource + ?Sized> ByteSource for Box<T> {
    fn size(&self) -> u64 {
        (**self).size()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        (**self).read_at(offset, buf)
    }
}

impl<T: ByteSource + ?Sized> ByteSource for Arc<T> {
    fn size(&self) -> u64 {
        (**self).size()
    }
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        (**self).read_at(offset, buf)
    }
}

/// Cursor over a [`ByteSource`] implementing [`Read`] and [`Seek`].
///
/// For consumers that expect a stream (parsers, hashers, `io::copy`).
/// Seeking past the end is allowed; reads there return `0`.
#[derive(Debug)]
pub struct SourceReader<S> {
    source: S,
    pos: u64,
}

impl<S: ByteSource> SourceReader<S> {
    /// Wraps `source` with the cursor at offset 0.
    pub fn new(source: S) -> Self {
        Self { source, pos: 0 }
    }

    /// Current cursor position.
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// The wrapped source.
    pub fn get_ref(&self) -> &S {
        &self.source
    }

    /// Returns the wrapped source.
    pub fn into_inner(self) -> S {
        self.source
    }
}

impl<S: ByteSource> Read for SourceReader<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.source.read_at(self.pos, buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl<S: ByteSource> Seek for SourceReader<S> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let (base, delta) = match to {
            SeekFrom::Start(n) => {
                self.pos = n;
                return Ok(n);
            }
            SeekFrom::End(d) => (self.source.size(), d),
            SeekFrom::Current(d) => (self.pos, d),
        };
        match base.checked_add_signed(delta) {
            Some(n) => {
                self.pos = n;
                Ok(n)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek to a negative or overflowing position",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory source that returns at most 3 bytes per call.
    #[derive(Debug)]
    struct Choppy(Vec<u8>);

    impl ByteSource for Choppy {
        fn size(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(self.0.len());
            let n = buf.len().min(3).min(self.0.len() - start);
            buf[..n].copy_from_slice(&self.0[start..start + n]);
            Ok(n)
        }
    }

    #[test]
    fn read_exact_at_retries_short_reads() {
        let src = Choppy((0..20).collect());
        let mut buf = [0u8; 10];
        src.read_exact_at(5, &mut buf).unwrap();
        assert_eq!(buf, [5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);
        let err = src.read_exact_at(15, &mut buf).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Other);
    }

    #[test]
    fn reader_reads_and_seeks() {
        let boxed: Box<dyn ByteSource> = Box::new(Choppy((0..20).collect()));
        let mut r = SourceReader::new(boxed);
        let mut all = Vec::new();
        r.read_to_end(&mut all).unwrap();
        assert_eq!(all, (0..20).collect::<Vec<u8>>());
        assert_eq!(r.seek(SeekFrom::End(-4)).unwrap(), 16);
        assert_eq!(r.seek(SeekFrom::Current(1)).unwrap(), 17);
        let mut b = [0u8; 1];
        r.read_exact(&mut b).unwrap();
        assert_eq!(b, [17]);
        assert!(r.seek(SeekFrom::Current(-100)).is_err());
        assert_eq!(r.seek(SeekFrom::Start(100)).unwrap(), 100);
        assert_eq!(r.read(&mut b).unwrap(), 0);
        assert_eq!(r.position(), 100);
    }
}
