// SPDX-License-Identifier: MPL-2.0
//! Background workers that fetch read-ahead blocks in parallel.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
use std::thread;

use crate::source::SourceShared;

/// One block to fetch: the source (weak, so a closed file cancels its jobs)
/// and the reader generation it was scheduled for.
pub(crate) struct Job {
    pub(crate) source: Weak<SourceShared>,
    pub(crate) chunk: u64,
    pub(crate) generation: u64,
}

struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
    workers: usize,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

/// A fixed set of worker threads per [`SmbFs`](crate::SmbFs), started on
/// the first job and stopped when the file system is dropped.
pub(crate) struct Prefetcher {
    shared: Arc<Shared>,
    max_workers: usize,
}

impl Prefetcher {
    pub(crate) fn new(max_workers: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                queue: Mutex::new(Queue {
                    jobs: VecDeque::new(),
                    closed: false,
                    workers: 0,
                }),
                ready: Condvar::new(),
            }),
            max_workers,
        }
    }

    /// `false` when background prefetch is off.
    pub(crate) fn enabled(&self) -> bool {
        self.max_workers > 0
    }

    /// Queues `jobs`, starting workers up to the limit.
    pub(crate) fn submit(&self, jobs: Vec<Job>) {
        if jobs.is_empty() {
            return;
        }
        let mut q = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if q.closed {
            return;
        }
        let n = jobs.len();
        q.jobs.extend(jobs);
        while q.workers < self.max_workers.min(q.jobs.len().max(n)) {
            let shared = Arc::clone(&self.shared);
            let spawned = thread::Builder::new()
                .name("pa-smb-prefetch".into())
                .spawn(move || worker(&shared));
            if spawned.is_err() {
                break; // Readers fetch synchronously instead.
            }
            q.workers += 1;
        }
        drop(q);
        self.shared.ready.notify_all();
    }

    /// Stops the workers after their current job; queued jobs are dropped.
    pub(crate) fn close(&self) {
        let mut q = self.shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        q.closed = true;
        let dropped: Vec<Job> = q.jobs.drain(..).collect();
        drop(q);
        self.shared.ready.notify_all();
        for job in dropped {
            if let Some(src) = job.source.upgrade() {
                src.cancel(job.chunk);
            }
        }
    }
}

fn worker(shared: &Shared) {
    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if q.closed {
                    q.workers -= 1;
                    return;
                }
                if let Some(job) = q.jobs.pop_front() {
                    break job;
                }
                q = shared.ready.wait(q).unwrap_or_else(PoisonError::into_inner);
            }
        };
        if let Some(src) = job.source.upgrade() {
            src.prefetch(job.chunk, job.generation);
        }
    }
}
