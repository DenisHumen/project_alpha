// SPDX-License-Identifier: MPL-2.0
//! Parallel directory walk over a [`Vfs`].
//!
//! Network listings are latency-bound (one SMB QUERY_DIRECTORY round trip
//! per folder), so several worker threads list folders at once and send
//! the filtered results to the caller's thread, which owns the database.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use pa_vfs::{EntryKind, Vfs, VfsError, VfsPath};

use crate::rules::ScanRules;

/// What a walk lists and records.
pub(crate) trait WalkFilter: Sync {
    /// Deepest folder level entered; the source root is level 0.
    fn max_depth(&self) -> usize;
    /// Whether the folder `name` inside `parent` is listed.
    fn enter(&self, parent: &VfsPath, name: &str) -> bool;
    /// Whether the file `name` is sent to the caller.
    fn keep(&self, name: &str) -> bool;
}

impl WalkFilter for ScanRules {
    fn max_depth(&self) -> usize {
        self.max_depth
    }
    fn enter(&self, parent: &VfsPath, name: &str) -> bool {
        self.enter_folder_in(parent.as_str(), name)
    }
    fn keep(&self, name: &str) -> bool {
        self.classify(name).is_some()
    }
}

/// A media file seen by the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeenFile {
    pub path: VfsPath,
    pub size: u64,
    pub modified_ms: Option<i64>,
}

/// Result of listing one folder.
#[derive(Debug)]
pub(crate) struct Listed {
    pub dir: VfsPath,
    pub result: Result<Vec<SeenFile>, VfsError>,
    /// Folders queued for listing after this one, in total so far.
    pub dirs_queued: usize,
}

struct State {
    queue: Vec<(VfsPath, usize)>,
    active: usize,
    queued_total: usize,
    done: bool,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// Runs the walk on `threads` workers inside `scope` and returns the stream
/// of listed folders, starting at `root` (at folder level `depth`; the
/// source root is level 0). The stream ends when every folder is listed or `stop`
/// is set (cancellation or a fatal error on the receiving side).
pub(crate) fn spawn<'scope, 'env>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    vfs: &'env dyn Vfs,
    rules: &'env dyn WalkFilter,
    root: VfsPath,
    depth: usize,
    threads: usize,
    stop: Arc<AtomicBool>,
) -> Receiver<Listed> {
    let threads = threads.max(1);
    // Bounded: workers wait when the database side falls behind.
    let (tx, rx) = sync_channel(threads * 4);
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            queue: vec![(root, depth)],
            active: 0,
            queued_total: 1,
            done: false,
        }),
        wake: Condvar::new(),
    });
    for _ in 0..threads {
        let tx = tx.clone();
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        scope.spawn(move || worker(vfs, rules, &shared, &stop, &tx));
    }
    rx
}

fn worker(
    vfs: &dyn Vfs,
    rules: &dyn WalkFilter,
    shared: &Shared,
    stop: &AtomicBool,
    tx: &SyncSender<Listed>,
) {
    loop {
        let Some((dir, depth)) = next_dir(shared, stop) else {
            return;
        };

        let mut subdirs = Vec::new();
        let result = vfs.list(&dir).map(|entries| {
            let mut files = Vec::new();
            for e in entries {
                match e.metadata.kind {
                    EntryKind::Dir => {
                        if depth < rules.max_depth() && rules.enter(&dir, &e.name) {
                            subdirs.push((e.path, depth + 1));
                        }
                    }
                    EntryKind::File if rules.keep(&e.name) => {
                        files.push(SeenFile {
                            path: e.path,
                            size: e.metadata.size,
                            modified_ms: e.metadata.modified_ms,
                        });
                    }
                    _ => {}
                }
            }
            files
        });

        let dirs_queued = {
            let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            st.queued_total += subdirs.len();
            st.queue.extend(subdirs);
            st.active -= 1;
            if st.active == 0 && st.queue.is_empty() {
                st.done = true;
            }
            shared.wake.notify_all();
            st.queued_total
        };

        if tx
            .send(Listed {
                dir,
                result,
                dirs_queued,
            })
            .is_err()
        {
            // The receiver is gone: stop everyone.
            stop.store(true, Ordering::Relaxed);
            shared.wake.notify_all();
            return;
        }
    }
}

/// Takes the next folder, waiting while other workers may still add some.
fn next_dir(shared: &Shared, stop: &AtomicBool) -> Option<(VfsPath, usize)> {
    let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if st.done || stop.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(next) = st.queue.pop() {
            st.active += 1;
            return Some(next);
        }
        if st.active == 0 {
            st.done = true;
            shared.wake.notify_all();
            return None;
        }
        // Timed wait so a cancellation is noticed without a notification.
        st = shared
            .wake
            .wait_timeout(st, Duration::from_millis(50))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}
