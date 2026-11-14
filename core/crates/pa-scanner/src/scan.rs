// SPDX-License-Identifier: MPL-2.0
//! The shallow incremental scan: walk, diff by `(path, size, mtime)`, write.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pa_library::rusqlite::{self, Connection, OptionalExtension, params};
use pa_vfs::{ErrorKind, Vfs, VfsError, VfsPath};

use crate::rules::ScanRules;
use crate::walk::{self, Listed, SeenFile};

/// Default time a file stays soft-deleted before a scan removes its row.
pub const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// How a scan runs.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// What to record and which folders to skip.
    pub rules: ScanRules,
    /// Folders listed in parallel. Network sources are latency-bound, so
    /// a few parallel listings speed them up several times.
    pub threads: usize,
    /// How long a file not seen by scans keeps its row (and its match,
    /// probe and hashes) before it is deleted. A share that comes back
    /// within this time restores everything as it was.
    pub grace_period: Duration,
    /// Database changes per transaction.
    pub batch_size: usize,
    /// Clock override, Unix milliseconds (tests). `None`: the system clock.
    pub now_ms: Option<i64>,
    /// Scan only this folder and everything below it: the incremental
    /// rescan after a change (LIB-005). Rows outside it are not read or
    /// written; a folder that no longer exists marks its rows missing.
    /// `None`: the whole source.
    pub subtree: Option<VfsPath>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            rules: ScanRules::default(),
            threads: 4,
            grace_period: DEFAULT_GRACE_PERIOD,
            batch_size: 500,
            now_ms: None,
            subtree: None,
        }
    }
}

/// Cancels a running scan from another thread. Clones share the flag.
///
/// A cancelled scan keeps what it already wrote and marks nothing missing:
/// files it did not reach are left as they were.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that is not cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation; the scan stops within about 100 ms plus one
    /// folder listing.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// `true` once [`cancel`](Self::cancel) was called.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Counters of a running scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanProgress {
    /// Folders listed so far.
    pub dirs_listed: usize,
    /// Folders found so far (listed, failed and still waiting).
    pub dirs_found: usize,
    /// Media files seen so far.
    pub files_seen: usize,
    /// New files so far.
    pub added: usize,
    /// Files whose size or mtime changed so far.
    pub changed: usize,
    /// The folder just listed.
    pub current_dir: VfsPath,
}

/// Reported to the caller's callback while a scan runs, on the scanning thread.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScanEvent {
    /// A folder was listed and its files are written (or queued for the
    /// next batch).
    Progress(ScanProgress),
    /// A folder below the root could not be listed. Its files are left as
    /// they were: not marked missing.
    DirFailed {
        /// The folder.
        path: VfsPath,
        /// Why, as a protocol-neutral kind.
        kind: ErrorKind,
        /// Human-readable message (no secrets).
        message: String,
    },
    /// [`index_music`](crate::index_music): tags of `done` of `total`
    /// files are read and written.
    TagsProgress {
        /// Files written so far.
        done: usize,
        /// Files to read in this run.
        total: usize,
    },
    /// [`index_music`](crate::index_music): the tags of a file could not
    /// be read; it is indexed from its path.
    TagsFailed {
        /// The file.
        path: VfsPath,
        /// Human-readable message (no secrets).
        message: String,
    },
}

/// What a scan did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Media files seen.
    pub files_seen: usize,
    /// New rows.
    pub added: usize,
    /// Rows whose size or mtime changed; their probe, hashes and
    /// fingerprint were reset for the next stages.
    pub changed: usize,
    /// Soft-deleted rows seen again unchanged.
    pub restored: usize,
    /// Rows seen with the same size and mtime.
    pub unchanged: usize,
    /// Rows not seen this time and soft-deleted now.
    pub marked_missing: usize,
    /// Rows soft-deleted earlier and still missing, within the grace period.
    pub still_missing: usize,
    /// Rows deleted because they were missing longer than the grace period.
    pub purged: usize,
    /// Folders listed.
    pub dirs_listed: usize,
    /// Folders that failed to list (see [`ScanEvent::DirFailed`]).
    pub dirs_failed: usize,
    /// The scan was cancelled; missing files were not processed.
    pub cancelled: bool,
    /// The scan's clock, Unix milliseconds (`scanned_at_ms`, `missing_since_ms`).
    pub now_ms: i64,
    /// Wall time.
    pub elapsed: Duration,
}

/// Why a scan failed. Failures of single folders are not errors: they are
/// reported as [`ScanEvent::DirFailed`].
#[derive(Debug)]
#[non_exhaustive]
pub enum ScanError {
    /// No row with this id in `sources`.
    SourceNotFound(i64),
    /// The source root could not be listed (share offline, bad
    /// credentials). Nothing was changed.
    SourceUnavailable(VfsError),
    /// A database error. Batches committed before it stay.
    Library(rusqlite::Error),
    /// A library operation refused its input (music indexing).
    Catalog(pa_library::Error),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceNotFound(id) => write!(f, "source {id} not found"),
            Self::SourceUnavailable(e) => write!(f, "source unavailable: {e}"),
            Self::Library(e) => write!(f, "library database: {e}"),
            Self::Catalog(e) => write!(f, "library: {e}"),
        }
    }
}

impl std::error::Error for ScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SourceNotFound(_) => None,
            Self::SourceUnavailable(e) => Some(e),
            Self::Library(e) => Some(e),
            Self::Catalog(e) => Some(e),
        }
    }
}

impl From<rusqlite::Error> for ScanError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Library(e)
    }
}

impl From<pa_library::Error> for ScanError {
    fn from(e: pa_library::Error) -> Self {
        match e {
            pa_library::Error::Sqlite(e) => Self::Library(e),
            e => Self::Catalog(e),
        }
    }
}

/// A row of `files` as the diff needs it.
#[derive(Debug, Clone, Copy)]
struct Known {
    id: i64,
    size: i64,
    mtime_ms: Option<i64>,
    missing_since_ms: Option<i64>,
}

enum Op {
    Insert(SeenFile),
    Change(i64, SeenFile),
    Restore(i64),
}

/// Scans the source `source_id` (a row of `sources`) through `vfs`, whose
/// root is that source's root.
///
/// 1. Lists every folder (in parallel, filtered by [`ScanRules`]) and
///    records each media file's `(path, size, mtime)` in `files`.
/// 2. Compares with the rows already there: new files are inserted,
///    files with another size or mtime are updated and their derived data
///    (probe, hashes, fingerprint) reset, unchanged files are not written.
/// 3. Unless cancelled: rows not seen are soft-deleted (`missing_since_ms`),
///    rows missing longer than [`ScanOptions::grace_period`] are deleted.
///    Rows under a folder that failed to list are left alone.
///
/// Blocking; run it on a worker thread. `on_event` is called on the
/// calling thread.
pub fn scan(
    conn: &mut Connection,
    source_id: i64,
    vfs: &dyn Vfs,
    options: &ScanOptions,
    cancel: &CancelToken,
    on_event: &mut dyn FnMut(&ScanEvent),
) -> Result<ScanReport, ScanError> {
    let started = Instant::now();
    let now = options.now_ms.unwrap_or_else(system_now_ms);

    let exists: Option<i64> = conn
        .query_row("SELECT id FROM sources WHERE id = ?1", [source_id], |r| {
            r.get(0)
        })
        .optional()?;
    if exists.is_none() {
        return Err(ScanError::SourceNotFound(source_id));
    }
    let start = options.subtree.clone().unwrap_or_else(VfsPath::root);
    let start_depth = start.components().count();
    if !start.is_root() && !subtree_scanned(&start, &options.rules) {
        // A folder the full scan skips (extras, hidden, too deep): nothing
        // of it is in `files`.
        return Ok(ScanReport {
            now_ms: now,
            elapsed: started.elapsed(),
            ..ScanReport::default()
        });
    }
    let mut known = load_known(conn, source_id)?;
    if !start.is_root() {
        known.retain(|path, _| is_under(path, &start));
    }

    let mut run = Run {
        conn,
        source_id,
        now,
        batch_size: options.batch_size.max(1),
        known,
        start: start.clone(),
        pending: Vec::new(),
        failed_dirs: Vec::new(),
        report: ScanReport {
            now_ms: now,
            ..ScanReport::default()
        },
        progress: ScanProgress::default(),
    };

    let stop = Arc::new(AtomicBool::new(false));
    let walked = std::thread::scope(|s| {
        let rx = walk::spawn(
            s,
            vfs,
            &options.rules,
            start,
            start_depth,
            options.threads,
            Arc::clone(&stop),
        );
        let result = loop {
            if cancel.is_cancelled() {
                run.report.cancelled = true;
                break Ok(());
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(listed) => {
                    if let Err(e) = run.on_listed(listed, on_event) {
                        break Err(e);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        // Stop the workers; dropping the receiver unblocks their sends.
        stop.store(true, Ordering::Relaxed);
        drop(rx);
        result
    });
    walked?;

    run.flush()?;
    if !run.report.cancelled {
        run.finish_missing(options.grace_period)?;
    }
    run.report.elapsed = started.elapsed();
    Ok(run.report)
}

struct Run<'c> {
    conn: &'c mut Connection,
    source_id: i64,
    now: i64,
    batch_size: usize,
    /// The first folder listed: the source root or [`ScanOptions::subtree`].
    start: VfsPath,
    /// Rows not seen yet in this scan, by path.
    known: HashMap<String, Known>,
    pending: Vec<Op>,
    failed_dirs: Vec<VfsPath>,
    report: ScanReport,
    progress: ScanProgress,
}

impl Run<'_> {
    fn on_listed(
        &mut self,
        listed: Listed,
        on_event: &mut dyn FnMut(&ScanEvent),
    ) -> Result<(), ScanError> {
        self.progress.dirs_found = listed.dirs_queued;
        let files = match listed.result {
            Ok(files) => files,
            Err(e) if listed.dir.is_root() => return Err(ScanError::SourceUnavailable(e)),
            // The rescanned folder is gone: its rows are marked missing.
            Err(e) if listed.dir == self.start && e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                self.report.dirs_failed += 1;
                on_event(&ScanEvent::DirFailed {
                    path: listed.dir.clone(),
                    kind: e.kind(),
                    message: e.message().to_owned(),
                });
                self.failed_dirs.push(listed.dir);
                return Ok(());
            }
        };

        self.report.dirs_listed += 1;
        for file in files {
            self.report.files_seen += 1;
            let size = to_i64(file.size);
            match self.known.remove(file.path.as_str()) {
                None => {
                    self.report.added += 1;
                    self.pending.push(Op::Insert(file));
                }
                Some(k) if k.size != size || k.mtime_ms != file.modified_ms => {
                    self.report.changed += 1;
                    self.pending.push(Op::Change(k.id, file));
                }
                Some(k) if k.missing_since_ms.is_some() => {
                    self.report.restored += 1;
                    self.pending.push(Op::Restore(k.id));
                }
                Some(_) => self.report.unchanged += 1,
            }
        }
        if self.pending.len() >= self.batch_size {
            self.flush()?;
        }

        self.progress.dirs_listed = self.report.dirs_listed;
        self.progress.files_seen = self.report.files_seen;
        self.progress.added = self.report.added;
        self.progress.changed = self.report.changed;
        self.progress.current_dir = listed.dir;
        on_event(&ScanEvent::Progress(self.progress.clone()));
        Ok(())
    }

    /// Writes the pending changes in one transaction.
    fn flush(&mut self) -> Result<(), ScanError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO files (source_id, path, size_bytes, mtime_ms, added_at_ms, scanned_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT (source_id, path) DO UPDATE SET
                     size_bytes = excluded.size_bytes,
                     mtime_ms = excluded.mtime_ms,
                     scanned_at_ms = excluded.scanned_at_ms,
                     missing_since_ms = NULL",
            )?;
            let mut change = tx.prepare_cached(
                "UPDATE files SET
                     size_bytes = ?2, mtime_ms = ?3, scanned_at_ms = ?4, missing_since_ms = NULL,
                     container = NULL, duration_ms = NULL, video_json = NULL,
                     audio_langs = NULL, sub_langs = NULL, embedded_tags_json = NULL,
                     fingerprint = NULL, oshash = NULL, ed2k = NULL, crc32 = NULL,
                     probed_at_ms = NULL, hashed_at_ms = NULL
                 WHERE id = ?1",
            )?;
            let mut restore = tx.prepare_cached(
                "UPDATE files SET missing_since_ms = NULL, scanned_at_ms = ?2 WHERE id = ?1",
            )?;
            for op in self.pending.drain(..) {
                match op {
                    Op::Insert(f) => {
                        insert.execute(params![
                            self.source_id,
                            f.path.as_str(),
                            to_i64(f.size),
                            f.modified_ms,
                            self.now
                        ])?;
                    }
                    Op::Change(id, f) => {
                        change.execute(params![id, to_i64(f.size), f.modified_ms, self.now])?;
                    }
                    Op::Restore(id) => {
                        restore.execute(params![id, self.now])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Soft-deletes rows not seen and purges those past the grace period.
    fn finish_missing(&mut self, grace: Duration) -> Result<(), ScanError> {
        let grace_ms = i64::try_from(grace.as_millis()).unwrap_or(i64::MAX);
        let purge_before = self.now.saturating_sub(grace_ms);
        let tx = self.conn.transaction()?;
        {
            let mut mark = tx.prepare("UPDATE files SET missing_since_ms = ?2 WHERE id = ?1")?;
            let mut purge = tx.prepare("DELETE FROM files WHERE id = ?1")?;
            for (path, k) in &self.known {
                if self.failed_dirs.iter().any(|d| is_under(path, d)) {
                    continue;
                }
                match k.missing_since_ms {
                    None => {
                        mark.execute(params![k.id, self.now])?;
                        self.report.marked_missing += 1;
                    }
                    Some(since) if since <= purge_before => {
                        purge.execute([k.id])?;
                        self.report.purged += 1;
                    }
                    Some(_) => self.report.still_missing += 1,
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn load_known(conn: &Connection, source_id: i64) -> Result<HashMap<String, Known>, ScanError> {
    let mut stmt = conn.prepare(
        "SELECT id, path, size_bytes, mtime_ms, missing_since_ms FROM files WHERE source_id = ?1",
    )?;
    let rows = stmt.query_map([source_id], |r| {
        Ok((
            r.get::<_, String>(1)?,
            Known {
                id: r.get(0)?,
                size: r.get(2)?,
                mtime_ms: r.get(3)?,
                missing_since_ms: r.get(4)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Whether the full scan enters every folder of `dir` (a subtree root).
fn subtree_scanned(dir: &VfsPath, rules: &ScanRules) -> bool {
    let mut depth = 0;
    for name in dir.components() {
        depth += 1;
        if depth > rules.max_depth || !rules.enter_folder(name) {
            return false;
        }
    }
    true
}

/// `path` is inside folder `dir` (any depth).
fn is_under(path: &str, dir: &VfsPath) -> bool {
    dir.is_root()
        || path
            .strip_prefix(dir.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}

fn to_i64(size: u64) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

pub(crate) fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_under_matches_whole_components() {
        let d = VfsPath::parse("/Movies/Dune").unwrap();
        assert!(is_under("/Movies/Dune/Dune.mkv", &d));
        assert!(is_under("/Movies/Dune/x/y.mkv", &d));
        assert!(!is_under("/Movies/Dune 2/Dune.mkv", &d));
        assert!(!is_under("/Movies/Dune", &d));
        assert!(is_under("/a.mkv", &VfsPath::root()));
    }
}
