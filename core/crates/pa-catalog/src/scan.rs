// SPDX-License-Identifier: MPL-2.0
//! The scan pipeline of one or all sources.
//!
//! Per source: list (`pa_scanner::scan`), read music tags
//! (`pa_scanner::index_music`), analyze the folder structure
//! (`pa_scanner::analyze_folders`), fingerprint new files, recognize new videos
//! ([`crate::recognize`]). Then orphans are removed and the search index is
//! brought up to date. The pipeline runs on its own database connection,
//! so queries on the catalog keep working while it runs (WAL).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pa_library::rusqlite::params;
use pa_scanner::{CancelToken, FolderOptions, MusicOptions, ScanError, ScanEvent, ScanOptions};
use pa_vfs::{LocalFs, VfsPath};

use crate::{Catalog, Error, sources};

/// Stage of a running scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanPhase {
    /// Opening the source root.
    Connecting,
    /// Listing folders.
    Scanning,
    /// Reading tags, fingerprinting and recognizing new files.
    Analyzing,
    /// Finished; errors were reported along the way.
    Done,
}

/// Progress of [`Catalog::scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanUpdate {
    pub phase: ScanPhase,
    pub source_id: Option<i64>,
    /// Media files seen so far (all sources).
    pub files_found: u64,
    /// Done in the current step.
    pub processed: u64,
    /// Total of the current step, if known.
    pub total: Option<u64>,
    pub current_path: Option<String>,
    /// An error of this step; the scan goes on with the next source.
    pub error: Option<String>,
}

impl ScanUpdate {
    fn new(phase: ScanPhase, source_id: Option<i64>, files_found: u64) -> Self {
        Self {
            phase,
            source_id,
            files_found,
            processed: 0,
            total: None,
            current_path: None,
            error: None,
        }
    }
}

/// Start, end and fatal error of one source's scan in the `sources` status
/// columns (NET-011), shown by the Sources screen (UI-005). Written on a
/// short connection of its own; the end is written on drop, so every exit
/// of the pipeline (errors, cancel) closes the scan. Status writes are best
/// effort: a failure never fails the scan.
struct ScanStatus<'a> {
    db: &'a Path,
    id: i64,
    error: Option<String>,
}

impl<'a> ScanStatus<'a> {
    fn start(db: &'a Path, id: i64) -> Self {
        Self::write(
            db,
            "UPDATE sources SET last_scan_started_at_ms = ?2, last_scan_finished_at_ms = NULL
             WHERE id = ?1",
            id,
            None,
        );
        Self {
            db,
            id,
            error: None,
        }
    }

    /// The source could not be scanned: `message` becomes its last error.
    fn fail(&mut self, message: String) {
        self.error = Some(message);
    }

    fn write(db: &Path, sql: &str, id: i64, error: Option<&str>) {
        if let Ok(conn) = pa_library::open(db) {
            let now = crate::now_ms();
            let _ = match error {
                Some(e) => conn.execute(sql, params![id, now, e]),
                None => conn.execute(sql, params![id, now]),
            };
        }
    }
}

impl Drop for ScanStatus<'_> {
    fn drop(&mut self) {
        match self.error.take() {
            Some(e) => Self::write(
                self.db,
                "UPDATE sources SET last_scan_finished_at_ms = ?2, last_error = ?3,
                     last_error_at_ms = ?2
                 WHERE id = ?1",
                self.id,
                Some(&e),
            ),
            None => Self::write(
                self.db,
                "UPDATE sources SET last_scan_finished_at_ms = ?2 WHERE id = ?1",
                self.id,
                None,
            ),
        }
    }
}

fn n(v: usize) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

impl Catalog {
    /// Scans source `source_id`, or every source. `roots` gives the local
    /// path of sources that are not `file://` (a mounted SMB share). Scans
    /// are serialized. Errors of one source are reported as updates and the
    /// scan goes on; the last update is [`ScanPhase::Done`].
    pub fn scan(
        &self,
        source_id: Option<i64>,
        roots: &HashMap<i64, PathBuf>,
        cancel: &CancelToken,
        on_update: &mut dyn FnMut(ScanUpdate),
    ) -> Result<(), Error> {
        let _serial = self.scan_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut conn = pa_library::open(&self.path)?;
        let all = sources::list(&conn)?;
        let selected: Vec<_> = match source_id {
            Some(id) => all.into_iter().filter(|s| s.id == id).collect(),
            None => all,
        };
        let mut found = 0u64;
        if let (Some(id), true) = (source_id, selected.is_empty()) {
            let mut u = ScanUpdate::new(ScanPhase::Done, Some(id), 0);
            u.error = Some(format!("unknown source {id}"));
            on_update(u);
            return Ok(());
        }
        for src in &selected {
            if cancel.is_cancelled() {
                break;
            }
            let fail = |found: u64, msg: String| {
                let mut u = ScanUpdate::new(ScanPhase::Connecting, Some(src.id), found);
                u.error = Some(format!("{}: {msg}", src.name));
                u
            };
            let mut status = ScanStatus::start(&self.path, src.id);
            let mut u = ScanUpdate::new(ScanPhase::Connecting, Some(src.id), found);
            u.current_path = Some(src.uri.clone());
            on_update(u);
            let Some(root) = roots.get(&src.id).cloned().or(src.local_root.clone()) else {
                status.fail("no local path for this source".into());
                on_update(fail(found, "no local path for this source".into()));
                continue;
            };
            if !root.is_dir() {
                // Offline drive / unmounted share: keep the items.
                let msg = format!("folder not found: {}", root.display());
                status.fail(msg.clone());
                on_update(fail(found, msg));
                continue;
            }
            let vfs = match LocalFs::new(&root) {
                Ok(v) => v,
                Err(e) => {
                    status.fail(e.to_string());
                    on_update(fail(found, e.to_string()));
                    continue;
                }
            };

            let base = found;
            let mut on_event = |e: &ScanEvent| match e {
                ScanEvent::Progress(p) => {
                    let mut u =
                        ScanUpdate::new(ScanPhase::Scanning, Some(src.id), base + n(p.files_seen));
                    u.current_path = Some(p.current_dir.to_string());
                    on_update(u);
                }
                ScanEvent::DirFailed { path, message, .. } => {
                    let mut u = ScanUpdate::new(ScanPhase::Scanning, Some(src.id), base);
                    u.error = Some(format!("{}: {path}: {message}", src.name));
                    on_update(u);
                }
                _ => {}
            };
            let report = match pa_scanner::scan(
                &mut conn,
                src.id,
                &vfs,
                &ScanOptions::default(),
                cancel,
                &mut on_event,
            ) {
                Ok(r) => r,
                Err(ScanError::SourceUnavailable(e)) => {
                    status.fail(e.to_string());
                    on_update(fail(found, e.to_string()));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            found += n(report.files_seen);
            if report.cancelled {
                break;
            }

            let mut on_tags = |e: &ScanEvent| {
                if let ScanEvent::TagsProgress { done, total } = e {
                    let mut u = ScanUpdate::new(ScanPhase::Analyzing, Some(src.id), found);
                    u.processed = n(*done);
                    u.total = Some(n(*total));
                    on_update(u);
                }
            };
            pa_scanner::index_music(
                &mut conn,
                src.id,
                &vfs,
                &MusicOptions::default(),
                cancel,
                &mut on_tags,
            )?;
            // LIB-007: folder structure (discs, parts, extras, sidecars,
            // NFO, local art) into `pa_library::groups`. A failure is
            // reported and the pipeline goes on.
            if let Err(e) = pa_scanner::analyze_folders(
                &mut conn,
                src.id,
                &vfs,
                &FolderOptions::default(),
                cancel,
                &mut |_| {},
            ) {
                let mut u = ScanUpdate::new(ScanPhase::Analyzing, Some(src.id), found);
                u.error = Some(format!("{}: folder analysis: {e}", src.name));
                on_update(u);
            }
            let mut report_step = |done: usize, total: usize| {
                let mut u = ScanUpdate::new(ScanPhase::Analyzing, Some(src.id), found);
                u.processed = n(done);
                u.total = Some(n(total));
                on_update(u);
            };
            crate::fingerprint::fill_missing(&mut conn, src.id, &vfs, &mut report_step)?;
            crate::recognize::run(&mut conn, src.id, &mut report_step)?;
        }
        {
            let tx = conn.transaction()?;
            crate::recognize::remove_orphans(&tx)?;
            tx.commit()?;
        }
        pa_library::search::sync(&conn)?;
        let mut done = ScanUpdate::new(ScanPhase::Done, None, found);
        done.processed = found;
        done.total = Some(found);
        on_update(done);
        Ok(())
    }
}

/// What [`Catalog::rescan_folders`] changed in `files`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RescanReport {
    /// Media files seen in the rescanned folders.
    pub files_seen: u64,
    /// New files.
    pub added: u64,
    /// Files whose size or mtime changed.
    pub changed: u64,
    /// Files seen again after they went missing.
    pub restored: u64,
    /// Files gone since the last scan (soft-deleted).
    pub removed: u64,
}

impl RescanReport {
    /// `true` if the media files of the library changed.
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.added + self.changed + self.restored + self.removed > 0
    }
}

impl Catalog {
    /// Incremental rescan of some folders of one source (the change
    /// watcher, LIB-005): lists only `folders` (paths relative to the
    /// source root, each with everything below it) and runs the rest of
    /// the pipeline on what changed: music tags, the folder structure of
    /// those folders, fingerprints and recognition of new files, the
    /// search index. `root` is the local path of the source root.
    ///
    /// Serialized with [`scan`](Self::scan). A root that is not a folder
    /// (offline drive, unmounted share) is an error and nothing changes.
    pub fn rescan_folders(
        &self,
        source_id: i64,
        root: &Path,
        folders: &[VfsPath],
        cancel: &CancelToken,
    ) -> Result<RescanReport, Error> {
        let _serial = self.scan_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut conn = pa_library::open(&self.path)?;
        if sources::get(&conn, source_id)?.is_none() {
            return Err(Error::NotFound(format!("source {source_id}")));
        }
        if !root.is_dir() {
            return Err(Error::Io(format!("folder not found: {}", root.display())));
        }
        let vfs = LocalFs::new(root).map_err(|e| Error::Io(e.to_string()))?;
        let mut out = RescanReport::default();
        for folder in folders {
            if cancel.is_cancelled() {
                return Ok(out);
            }
            let options = ScanOptions {
                subtree: Some(folder.clone()),
                ..ScanOptions::default()
            };
            let r = pa_scanner::scan(&mut conn, source_id, &vfs, &options, cancel, &mut |_| {})?;
            out.files_seen += n(r.files_seen);
            out.added += n(r.added);
            out.changed += n(r.changed);
            out.restored += n(r.restored);
            out.removed += n(r.marked_missing);
        }
        if out.has_changes() {
            pa_scanner::index_music(
                &mut conn,
                source_id,
                &vfs,
                &MusicOptions::default(),
                cancel,
                &mut |_| {},
            )?;
        }
        // Sidecars, NFO and art can change without a media file.
        for folder in folders {
            analyze_subtree(&mut conn, source_id, &vfs, folder, cancel);
        }
        if out.has_changes() {
            crate::fingerprint::fill_missing(&mut conn, source_id, &vfs, &mut |_, _| {})?;
            crate::recognize::run(&mut conn, source_id, &mut |_, _| {})?;
            {
                let tx = conn.transaction()?;
                crate::recognize::remove_orphans(&tx)?;
                tx.commit()?;
            }
            pa_library::search::sync(&conn)?;
        }
        Ok(out)
    }
}

/// Folder structure of one subtree. A failure leaves the groups as they
/// were until the next full scan.
fn analyze_subtree(
    conn: &mut pa_library::rusqlite::Connection,
    source_id: i64,
    vfs: &LocalFs,
    folder: &VfsPath,
    cancel: &CancelToken,
) {
    let options = FolderOptions {
        subtree: Some(folder.clone()),
        ..FolderOptions::default()
    };
    let _ = pa_scanner::analyze_folders(conn, source_id, vfs, &options, cancel, &mut |_| {});
}
