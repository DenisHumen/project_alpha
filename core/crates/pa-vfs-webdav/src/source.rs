// SPDX-License-Identifier: MPL-2.0
//! An opened WebDAV file: ranged GETs with a small read-ahead cache.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read};
use std::sync::{Arc, Mutex, PoisonError};

use pa_vfs::{ByteSource, ErrorKind, Result, VfsError};
use reqwest::header::CONTENT_RANGE;
use reqwest::{Method, StatusCode, Url};

use crate::client::{Inner, Req, status_error};

/// [`ByteSource`] over HTTP `Range` requests.
///
/// A read smaller than the read-ahead block fetches a whole block and keeps
/// it; the next sequential reads are served from memory. Up to
/// `cache_blocks` blocks are kept (most recently used first), so a demuxer
/// reading the start and a thread reading the index at the end do not evict
/// each other. The lock is not held during network I/O: threads fetch in
/// parallel over pooled connections.
pub(crate) struct WebDavSource {
    fs: Arc<Inner>,
    /// Updated to the final location after a redirect, so later reads skip
    /// the extra hop.
    url: Mutex<Url>,
    name: String,
    size: u64,
    blocks: Mutex<VecDeque<(u64, Arc<[u8]>)>>,
}

impl fmt::Debug for WebDavSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebDavSource")
            .field("path", &self.name)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl WebDavSource {
    pub(crate) fn new(fs: Arc<Inner>, url: Url, name: String, size: u64) -> Self {
        Self {
            fs,
            url: Mutex::new(url),
            name,
            size,
            blocks: Mutex::new(VecDeque::new()),
        }
    }

    fn cached(&self, offset: u64, buf: &mut [u8]) -> Option<usize> {
        let mut blocks = self.blocks.lock().unwrap_or_else(PoisonError::into_inner);
        let i = blocks
            .iter()
            .position(|(start, data)| offset >= *start && offset - start < data.len() as u64)?;
        let (start, data) = blocks.remove(i)?;
        let n = copy_from(&data, offset - start, buf);
        blocks.push_front((start, data));
        Some(n)
    }

    /// GETs `len` bytes at `offset`; returns fewer only at the end of the
    /// resource.
    fn fetch(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
        let url = self
            .url
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut req = Req::new(Method::GET, url.clone());
        req.range = Some((offset, offset + len as u64 - 1));
        let mut resp = self.fs.send(&req, &self.name)?;
        if *resp.url() != url && resp.status().is_success() {
            *self.url.lock().unwrap_or_else(PoisonError::into_inner) = resp.url().clone();
        }
        let skip = match resp.status() {
            StatusCode::PARTIAL_CONTENT => {
                let start = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(content_range_start);
                if start.is_some_and(|s| s != offset) {
                    return Err(VfsError::new(
                        ErrorKind::Other,
                        format!("{}: server returned a different range", self.name),
                    ));
                }
                0
            }
            // The server ignores Range: skip to the offset in the full body.
            StatusCode::OK => offset,
            // The file shrank since it was opened.
            StatusCode::RANGE_NOT_SATISFIABLE => return Ok(Vec::new()),
            s => return Err(status_error(s, &self.name)),
        };
        let io_err = |e: io::Error| VfsError::from_io(e, &self.name);
        if skip > 0 {
            let skipped = io::copy(&mut (&mut resp).take(skip), &mut io::sink()).map_err(io_err)?;
            if skipped < skip {
                return Ok(Vec::new());
            }
        }
        let mut data = Vec::with_capacity(len);
        resp.take(len as u64)
            .read_to_end(&mut data)
            .map_err(io_err)?;
        Ok(data)
    }
}

fn copy_from(data: &[u8], at: u64, buf: &mut [u8]) -> usize {
    let at = usize::try_from(at).unwrap_or(data.len()).min(data.len());
    let n = buf.len().min(data.len() - at);
    buf[..n].copy_from_slice(&data[at..at + n]);
    n
}

/// `bytes 100-199/1000` → `100`.
fn content_range_start(v: &str) -> Option<u64> {
    let range = v.trim().strip_prefix("bytes")?.trim_start();
    range.split('-').next()?.trim().parse().ok()
}

impl ByteSource for WebDavSource {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() || offset >= self.size {
            return Ok(0);
        }
        let left = self.size - offset;
        let want = usize::try_from(left).map_or(buf.len(), |l| l.min(buf.len()));
        let buf = &mut buf[..want];
        if let Some(n) = self.cached(offset, buf) {
            return Ok(n);
        }
        let ra = self.fs.read_ahead;
        if want >= ra {
            // Large reads go straight into the caller's buffer.
            let data = self.fetch(offset, want)?;
            return Ok(copy_from(&data, 0, buf));
        }
        let block = usize::try_from(left).map_or(ra, |l| l.min(ra));
        let data: Arc<[u8]> = self.fetch(offset, block)?.into();
        let n = copy_from(&data, 0, buf);
        if !data.is_empty() {
            let mut blocks = self.blocks.lock().unwrap_or_else(PoisonError::into_inner);
            blocks.push_front((offset, data));
            blocks.truncate(self.fs.cache_blocks);
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range() {
        assert_eq!(content_range_start("bytes 100-199/1000"), Some(100));
        assert_eq!(content_range_start("bytes 0-0/*"), Some(0));
        assert_eq!(content_range_start("items 1-2"), None);
    }

    #[test]
    fn copies_within_bounds() {
        let mut buf = [0u8; 4];
        assert_eq!(copy_from(b"abcdef", 4, &mut buf), 2);
        assert_eq!(&buf[..2], b"ef");
        assert_eq!(copy_from(b"abc", 9, &mut buf), 0);
    }
}
