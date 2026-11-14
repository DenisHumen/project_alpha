// SPDX-License-Identifier: MPL-2.0
//! The structural analysis pass over a source (LIB-007).
//!
//! Runs after [`scan`](crate::scan): lists the source again — this time
//! with extras, sample, subtitle and disc folders and the sidecar files
//! the shallow scan skips — takes the durations `pa-probe` stored in
//! `files`, analyzes the listing with [`analyze`] and stores the groups
//! with [`pa_library::groups::save_groups`].

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use pa_library::groups::{Role, save_groups};
use pa_library::rusqlite::{Connection, OptionalExtension};
use pa_vfs::{Vfs, VfsPath};

use crate::scan::{CancelToken, ScanError, ScanEvent, ScanProgress, system_now_ms};
use crate::structure::{ListedFile, StructureOptions, analyze, is_structure_file, parse_nfo_ids};
use crate::walk::{self, WalkFilter};

/// NFO files larger than this are read only up to it.
const NFO_READ_LIMIT: usize = 256 * 1024;

/// How [`analyze_folders`] runs.
#[derive(Debug, Clone)]
pub struct FolderOptions {
    /// Rules of the analysis (the optional duration rule).
    pub structure: StructureOptions,
    /// Folders listed in parallel.
    pub threads: usize,
    /// Deepest folder level entered; the source root is level 0.
    pub max_depth: usize,
    /// Skip names starting with `.`.
    pub skip_hidden: bool,
    /// Open NFO files and read the external ids in them.
    pub read_nfo: bool,
    /// Clock override, Unix milliseconds (tests).
    pub now_ms: Option<i64>,
}

impl Default for FolderOptions {
    fn default() -> Self {
        Self {
            structure: StructureOptions::default(),
            threads: 4,
            max_depth: 12,
            skip_hidden: true,
            read_nfo: true,
            now_ms: None,
        }
    }
}

/// What [`analyze_folders`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderReport {
    /// Files of the listing used by the analysis.
    pub files_listed: usize,
    /// Folders listed.
    pub dirs_listed: usize,
    /// Folders that failed to list; their groups were left as they were.
    pub dirs_failed: usize,
    /// Groups found.
    pub groups: usize,
    /// Groups inserted or rewritten.
    pub written: usize,
    /// Groups unchanged since the last run (nothing written).
    pub unchanged: usize,
    /// Groups deleted (their folders are gone or no longer form a group).
    pub removed: usize,
    /// NFO files read.
    pub nfo_read: usize,
    /// Cancelled: nothing was written.
    pub cancelled: bool,
    /// Wall time.
    pub elapsed: Duration,
}

/// The walk of the analysis: everything the analysis uses, including the
/// folders the shallow scan skips. Inside `BDMV/` only `STREAM/` is listed.
struct StructureWalk<'a> {
    options: &'a FolderOptions,
}

/// NAS and OS service folders, never listed.
const SERVICE_FOLDERS: &[&str] = &[
    "@eadir",
    "#recycle",
    "#snapshot",
    "$recycle.bin",
    "system volume information",
    "lost+found",
    ".@__thumb",
    "@recycle",
    ".trash",
    ".trashes",
];

impl WalkFilter for StructureWalk<'_> {
    fn max_depth(&self) -> usize {
        self.options.max_depth
    }

    fn enter(&self, parent: &VfsPath, name: &str) -> bool {
        if self.options.skip_hidden && name.starts_with('.') {
            return false;
        }
        let lower = name.to_lowercase();
        if SERVICE_FOLDERS.contains(&lower.as_str()) {
            return false;
        }
        let parent_name = parent.as_str().rsplit('/').next().unwrap_or("");
        if parent_name.eq_ignore_ascii_case("bdmv") {
            return lower == "stream";
        }
        // Extras, sample, subtitle and disc folders are listed (the shallow
        // scan skips them); CERTIFICATE / AUDIO_TS hold nothing needed.
        !matches!(lower.as_str(), "certificate" | "audio_ts")
    }

    fn keep(&self, name: &str) -> bool {
        !(self.options.skip_hidden && name.starts_with('.')) && is_structure_file(name)
    }
}

/// Analyzes the folders of source `source_id` (a row of `sources`) through
/// `vfs` and stores the groups (`file_groups`, `file_group_members`,
/// `group_assets`).
///
/// Run it after [`scan`](crate::scan) (members link to `files` rows by
/// path) and, for the optional duration rule, after `pa-probe`. Groups
/// under folders that fail to list are kept; a cancelled run writes
/// nothing. Unchanged groups are not written.
pub fn analyze_folders(
    conn: &mut Connection,
    source_id: i64,
    vfs: &dyn Vfs,
    options: &FolderOptions,
    cancel: &CancelToken,
    on_event: &mut dyn FnMut(&ScanEvent),
) -> Result<FolderReport, ScanError> {
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

    let mut report = FolderReport::default();
    let mut listing = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut progress = ScanProgress::default();
    let filter = StructureWalk { options };
    let stop = Arc::new(AtomicBool::new(false));
    let walked = std::thread::scope(|s| {
        let rx = walk::spawn(
            s,
            vfs,
            &filter,
            VfsPath::root(),
            options.threads,
            Arc::clone(&stop),
        );
        let result = loop {
            if cancel.is_cancelled() {
                report.cancelled = true;
                break Ok(());
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(listed) => match listed.result {
                    Ok(files) => {
                        report.dirs_listed += 1;
                        listing.extend(files);
                        progress.dirs_listed = report.dirs_listed;
                        progress.dirs_found = listed.dirs_queued;
                        progress.files_seen = listing.len();
                        progress.current_dir = listed.dir;
                        on_event(&ScanEvent::Progress(progress.clone()));
                    }
                    Err(e) if listed.dir.is_root() => break Err(ScanError::SourceUnavailable(e)),
                    Err(e) => {
                        report.dirs_failed += 1;
                        on_event(&ScanEvent::DirFailed {
                            path: listed.dir.clone(),
                            kind: e.kind(),
                            message: e.message().to_owned(),
                        });
                        failed.push(listed.dir.as_str().to_owned());
                    }
                },
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        stop.store(true, Ordering::Relaxed);
        drop(rx);
        result
    });
    walked?;
    if report.cancelled {
        report.elapsed = started.elapsed();
        return Ok(report);
    }

    let durations = load_durations(conn, source_id)?;
    let files: Vec<ListedFile> = listing
        .into_iter()
        .map(|f| {
            let path = f.path.as_str().to_owned();
            ListedFile {
                duration_ms: durations.get(&path).copied(),
                path,
                size: f.size,
            }
        })
        .collect();
    report.files_listed = files.len();

    let mut groups = analyze(&files, &options.structure);
    if options.read_nfo {
        for e in groups
            .iter_mut()
            .flat_map(|g| g.entries.iter_mut())
            .filter(|e| e.role == Role::Nfo)
        {
            if cancel.is_cancelled() {
                report.cancelled = true;
                report.elapsed = started.elapsed();
                return Ok(report);
            }
            if let Some(text) = read_text(vfs, &e.path) {
                report.nfo_read += 1;
                e.ids = parse_nfo_ids(&text);
            }
        }
    }
    report.groups = groups.len();
    let saved = save_groups(conn, source_id, &groups, &failed, now)?;
    report.written = saved.written;
    report.unchanged = saved.unchanged;
    report.removed = saved.removed;
    report.elapsed = started.elapsed();
    Ok(report)
}

fn load_durations(conn: &Connection, source_id: i64) -> Result<HashMap<String, u64>, ScanError> {
    let mut stmt = conn.prepare(
        "SELECT path, duration_ms FROM files WHERE source_id = ?1 AND duration_ms IS NOT NULL",
    )?;
    let rows = stmt.query_map([source_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, d) = row?;
        if let Ok(d) = u64::try_from(d) {
            out.insert(path, d);
        }
    }
    Ok(out)
}

/// The start of a text file (lossy UTF-8), or `None` if it cannot be read.
fn read_text(vfs: &dyn Vfs, path: &str) -> Option<String> {
    let path = VfsPath::parse(path).ok()?;
    let file = vfs.open(&path).ok()?;
    let len = usize::try_from(file.size())
        .unwrap_or(usize::MAX)
        .min(NFO_READ_LIMIT);
    let mut buf = vec![0u8; len];
    file.read_exact_at(0, &mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}
