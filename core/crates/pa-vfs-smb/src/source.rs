// SPDX-License-Identifier: MPL-2.0
//! [`SmbSource`]: positional reads with read-ahead and parallel prefetch.
//!
//! Random reads (a prober seeking to the MKV cues, a hasher reading 64 KiB
//! at both ends) go straight to the server. When a read starts where the
//! previous one ended, the file is being streamed: reads are then served
//! from blocks of `chunk_size` bytes, and the following blocks, up to the
//! read-ahead buffer, are fetched in the background, several in parallel on
//! separate connections. A non-sequential read bumps the generation, so
//! queued blocks of the old position are skipped.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use pa_vfs::{ByteSource, Result};

use crate::fs::FsInner;
use crate::pool::Conn;
use crate::prefetch::Job;

/// An open SMB file.
pub struct SmbSource {
    shared: Arc<SourceShared>,
}

impl fmt::Debug for SmbSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SmbSource({}, {} bytes)",
            self.shared.url, self.shared.size
        )
    }
}

pub(crate) struct SourceShared {
    fs: Arc<FsInner>,
    path: String,
    url: String,
    size: u64,
    chunk: u64,
    capacity: usize,
    state: Mutex<State>,
    changed: Condvar,
}

struct State {
    blocks: HashMap<u64, Arc<Vec<u8>>>,
    lru: VecDeque<u64>,
    inflight: HashSet<u64>,
    /// End of the previous read; a read starting here is sequential.
    next_offset: u64,
    generation: u64,
}

impl SmbSource {
    pub(crate) fn new(fs: Arc<FsInner>, path: String, url: String, size: u64) -> Self {
        let cfg = fs.pool.config();
        let chunk = cfg.chunk_size as u64;
        let capacity = cfg.capacity_chunks();
        Self {
            shared: Arc::new(SourceShared {
                fs,
                path,
                url,
                size,
                chunk,
                capacity,
                state: Mutex::new(State {
                    blocks: HashMap::new(),
                    lru: VecDeque::new(),
                    inflight: HashSet::new(),
                    next_offset: u64::MAX,
                    generation: 0,
                }),
                changed: Condvar::new(),
            }),
        }
    }
}

impl SourceShared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn read_ahead_on(&self) -> bool {
        self.capacity >= 2
    }

    /// Reads `[offset, offset + buf.len())` directly, one SMB READ.
    fn read_direct(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let len = usize::try_from(self.size - offset).map_or(buf.len(), |rest| rest.min(buf.len()));
        let buf = &mut buf[..len];
        self.fs
            .pool
            .with_conn(&self.url, |c| read_on(c, &self.path, offset, buf, false))
    }

    /// Fetches one whole block (several SMB READs if the server limits the
    /// request size).
    fn fetch_block(&self, idx: u64) -> Result<Vec<u8>> {
        let start = idx * self.chunk;
        let len = usize::try_from((self.size - start).min(self.chunk)).unwrap_or(0);
        let mut data = vec![0u8; len];
        self.fs.pool.with_conn(&self.url, |c| {
            read_on(c, &self.path, start, &mut data, true)
        })?;
        Ok(data)
    }

    fn insert(&self, st: &mut State, idx: u64, data: Vec<u8>) -> Arc<Vec<u8>> {
        let data = Arc::new(data);
        st.inflight.remove(&idx);
        st.blocks.insert(idx, Arc::clone(&data));
        st.lru.retain(|&i| i != idx);
        st.lru.push_back(idx);
        while st.blocks.len() > self.capacity {
            match st.lru.pop_front() {
                Some(old) => {
                    st.blocks.remove(&old);
                }
                None => break,
            }
        }
        data
    }

    /// Worker side: fetch a queued block unless the reader moved away.
    pub(crate) fn prefetch(&self, idx: u64, generation: u64) {
        if self.lock().generation != generation {
            self.cancel(idx);
            return;
        }
        let res = self.fetch_block(idx);
        let mut st = self.lock();
        match res {
            Ok(data) => {
                self.insert(&mut st, idx, data);
            }
            // The reader retries synchronously and gets the error itself.
            Err(_) => {
                st.inflight.remove(&idx);
            }
        }
        drop(st);
        self.changed.notify_all();
    }

    /// Drops the in-flight mark of a block that will not be fetched.
    pub(crate) fn cancel(&self, idx: u64) {
        self.lock().inflight.remove(&idx);
        self.changed.notify_all();
    }

    /// Gets block `idx` from the cache, waiting for a fetch in flight or
    /// fetching it on this thread.
    fn block(&self, idx: u64) -> Result<Arc<Vec<u8>>> {
        let mut st = self.lock();
        loop {
            if let Some(b) = st.blocks.get(&idx) {
                let b = Arc::clone(b);
                st.lru.retain(|&i| i != idx);
                st.lru.push_back(idx);
                return Ok(b);
            }
            if !st.inflight.contains(&idx) {
                break;
            }
            st = self
                .changed
                .wait(st)
                .unwrap_or_else(PoisonError::into_inner);
        }
        st.inflight.insert(idx);
        drop(st);
        let res = self.fetch_block(idx);
        let mut st = self.lock();
        let out = match res {
            Ok(data) => Ok(self.insert(&mut st, idx, data)),
            Err(e) => {
                st.inflight.remove(&idx);
                Err(e)
            }
        };
        drop(st);
        self.changed.notify_all();
        out
    }

    /// Queues the blocks after `idx` that are neither cached nor in flight.
    fn schedule(self: &Arc<Self>, idx: u64, generation: u64) {
        let prefetch = &self.fs.prefetch;
        if !prefetch.enabled() {
            return;
        }
        let last = self.size.saturating_sub(1) / self.chunk;
        let depth = (self.capacity - 1) as u64;
        let mut jobs = Vec::new();
        {
            let mut st = self.lock();
            for j in idx + 1..=idx.saturating_add(depth).min(last) {
                if st.blocks.contains_key(&j) || st.inflight.contains(&j) {
                    continue;
                }
                st.inflight.insert(j);
                jobs.push(Job {
                    source: Arc::downgrade(self),
                    chunk: j,
                    generation,
                });
            }
        }
        prefetch.submit(jobs);
    }
}

/// Reads into `buf` at `offset` on one connection. `fill` loops until the
/// buffer is full or the file ends; otherwise one READ (a short read is
/// fine for `ByteSource`).
fn read_on(
    c: &mut Conn,
    path: &str,
    offset: u64,
    buf: &mut [u8],
    fill: bool,
) -> std::result::Result<usize, pa_smb2::Error> {
    let file = c.file(path)?;
    let mut done = 0;
    while done < buf.len() {
        match c.ctx.pread(file, offset + done as u64, &mut buf[done..]) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) => {
                c.forget(path);
                return Err(e);
            }
        }
        if !fill {
            break;
        }
    }
    Ok(done)
}

impl ByteSource for SmbSource {
    fn size(&self) -> u64 {
        self.shared.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        let s = &self.shared;
        if buf.is_empty() || offset >= s.size {
            return Ok(0);
        }
        let idx = offset / s.chunk;
        let (sequential, cached, generation) = {
            let mut st = s.lock();
            let sequential = offset == st.next_offset;
            if !sequential {
                st.generation += 1;
            }
            (sequential, st.blocks.contains_key(&idx), st.generation)
        };
        let n = if s.read_ahead_on() && (sequential || cached) {
            let block = s.block(idx)?;
            let from = usize::try_from(offset - idx * s.chunk).unwrap_or(0);
            let n = buf.len().min(block.len().saturating_sub(from));
            buf[..n].copy_from_slice(&block[from..from + n]);
            if sequential {
                s.schedule(idx, generation);
            }
            n
        } else {
            s.read_direct(offset, buf)?
        };
        s.lock().next_offset = offset + n as u64;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    // The read paths need a server: covered by the contract suite against
    // Samba (tests/contract_servers.rs). Block arithmetic is checked here.

    #[test]
    fn block_ranges() {
        let chunk = 1024u64;
        let size = 3000u64;
        let last = size.saturating_sub(1) / chunk;
        assert_eq!(last, 2);
        let len = |idx: u64| (size - idx * chunk).min(chunk);
        assert_eq!((len(0), len(1), len(2)), (1024, 1024, 952));
    }
}
