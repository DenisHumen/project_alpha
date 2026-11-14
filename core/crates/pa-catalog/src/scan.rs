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
use std::path::PathBuf;

use pa_scanner::{CancelToken, FolderOptions, MusicOptions, ScanError, ScanEvent, ScanOptions};
use pa_vfs::LocalFs;

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
            let mut u = ScanUpdate::new(ScanPhase::Connecting, Some(src.id), found);
            u.current_path = Some(src.uri.clone());
            on_update(u);
            let Some(root) = roots.get(&src.id).cloned().or(src.local_root.clone()) else {
                on_update(fail(found, "no local path for this source".into()));
                continue;
            };
            if !root.is_dir() {
                // Offline drive / unmounted share: keep the items.
                on_update(fail(found, format!("folder not found: {}", root.display())));
                continue;
            }
            let vfs = match LocalFs::new(&root) {
                Ok(v) => v,
                Err(e) => {
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
