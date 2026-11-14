// SPDX-License-Identifier: MPL-2.0
//! The change watcher of the library (LIB-005).
//!
//! [`LibraryWatcher`] follows the auto-scan sources of a [`Catalog`]
//! ([`Source::auto_scan`](crate::Source::auto_scan)) on a background
//! thread and keeps the library in step with them:
//!
//! - **Local folders** are watched with the OS notification API through
//!   the `notify` crate (inotify on Linux and Android, FSEvents on macOS,
//!   ReadDirectoryChangesW on Windows). Events are debounced: a folder is
//!   rescanned once its events have been quiet for
//!   [`WatchOptions::debounce`], and at the latest
//!   [`WatchOptions::max_delay`] after the first one, so a long copy still
//!   shows up while it runs. Only the folders that changed are rescanned
//!   ([`Catalog::rescan_folders`]); a burst touching more than
//!   [`WatchOptions::max_folders`] folders, an event queue overflow or a
//!   change at the source root rescans the whole source.
//! - **Network sources** (an SMB share mounted by the app) get no reliable
//!   change events through a mount, so they are rescanned on a timer
//!   ([`WatchOptions::network_interval`]). A local folder that cannot be
//!   watched (inotify watch limit, a network file system) falls back to
//!   the timer too.
//! - On start every followed source is scanned once
//!   ([`WatchOptions::scan_on_start`]): changes made while the app was
//!   closed come in at once.
//!
//! Each rescan is reported as a [`WatchEvent`]; the app refreshes its views
//! on it. Rescans are serialized with [`Catalog::scan`]. Dropping the
//! watcher (or [`LibraryWatcher::stop`]) cancels a running rescan and
//! stops the thread.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, EventKind, ModifyKind, MetadataKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use pa_scanner::CancelToken;
use pa_vfs::VfsPath;

use crate::scan::{RescanReport, ScanPhase};
use crate::{Catalog, Error};

/// How a [`LibraryWatcher`] runs.
#[derive(Debug, Clone)]
pub struct WatchOptions {
    /// Quiet time after the last event of a folder before it is rescanned.
    pub debounce: Duration,
    /// Longest wait after the first event of a burst.
    pub max_delay: Duration,
    /// Rescan interval of network sources and of folders that cannot be
    /// watched. `None`: no timer, only [`scan_on_start`](Self::scan_on_start).
    pub network_interval: Option<Duration>,
    /// Scan every followed source when the watcher starts.
    pub scan_on_start: bool,
    /// More changed folders than this in one burst: the whole source is
    /// rescanned instead.
    pub max_folders: usize,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(1000),
            max_delay: Duration::from_secs(3),
            network_interval: Some(Duration::from_secs(15 * 60)),
            scan_on_start: true,
            max_folders: 32,
        }
    }
}

/// How a source is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchMode {
    /// OS change notifications.
    Events,
    /// Rescans on [`WatchOptions::network_interval`].
    Timer,
}

/// A source the watcher follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedSource {
    pub source_id: i64,
    /// Local path of the source root.
    pub root: PathBuf,
    pub mode: WatchMode,
}

/// A rescan done by the watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    pub source_id: i64,
    /// The whole source was scanned (start, timer, overflow, many folders).
    pub full: bool,
    /// Rescanned folders, relative to the source root (empty if `full`).
    pub folders: Vec<String>,
    /// What changed (incremental rescans only; zero after a full scan).
    pub report: RescanReport,
    /// Why the rescan or the watch failed (no secrets).
    pub error: Option<String>,
}

impl WatchEvent {
    fn new(source_id: i64, full: bool) -> Self {
        Self {
            source_id,
            full,
            folders: Vec::new(),
            report: RescanReport::default(),
            error: None,
        }
    }

    /// Whether the library may have changed: a full scan, or a rescan that
    /// found changes.
    #[must_use]
    pub fn may_have_changed(&self) -> bool {
        self.full || self.report.has_changes()
    }
}

/// Callback of the watcher, called on its thread.
pub type WatchCallback = Box<dyn FnMut(WatchEvent) + Send>;

enum Msg {
    Fs(notify::Result<notify::Event>),
    Stop,
}

/// Follows the auto-scan sources of a catalog; see the [module docs](self).
pub struct LibraryWatcher {
    tx: Sender<Msg>,
    cancel: CancelToken,
    thread: Option<JoinHandle<()>>,
    sources: Vec<WatchedSource>,
}

impl std::fmt::Debug for LibraryWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryWatcher")
            .field("sources", &self.sources)
            .finish_non_exhaustive()
    }
}

impl LibraryWatcher {
    /// Starts following the auto-scan sources of `catalog`. `roots` gives
    /// the local path of sources that are not `file://` (a mounted SMB
    /// share); a network source without one is skipped. `on_event` is
    /// called on the watcher thread after every rescan.
    pub fn start(
        catalog: Arc<Catalog>,
        roots: &HashMap<i64, PathBuf>,
        options: WatchOptions,
        mut on_event: WatchCallback,
    ) -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel();
        let fs_tx = tx.clone();
        let mut watcher = notify::recommended_watcher(move |e| {
            let _ = fs_tx.send(Msg::Fs(e));
        })
        .ok();

        let mut sources = Vec::new();
        let mut failures = Vec::new();
        for s in catalog.sources()? {
            if !s.auto_scan {
                continue;
            }
            let Some(root) = roots.get(&s.id).cloned().or(s.local_root.clone()) else {
                continue;
            };
            let mode = if s.kind == "local" {
                match watcher.as_mut().map(|w| w.watch(&root, RecursiveMode::Recursive)) {
                    Some(Ok(())) => WatchMode::Events,
                    Some(Err(e)) => {
                        failures.push((s.id, format!("{}: cannot watch: {e}", s.name)));
                        WatchMode::Timer
                    }
                    None => WatchMode::Timer,
                }
            } else {
                WatchMode::Timer
            };
            sources.push(WatchedSource {
                source_id: s.id,
                root,
                mode,
            });
        }
        for (id, message) in failures {
            let mut e = WatchEvent::new(id, false);
            e.error = Some(message);
            on_event(e);
        }

        let cancel = CancelToken::new();
        let run = Run {
            catalog,
            roots: roots.clone(),
            sources: sources
                .iter()
                .map(|s| Followed::new(s.clone(), &options))
                .collect(),
            options,
            cancel: cancel.clone(),
            on_event,
            _watcher: watcher,
        };
        let thread = std::thread::Builder::new()
            .name("pa-library-watch".into())
            .spawn(move || run.run(&rx))
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Self {
            tx,
            cancel,
            thread: Some(thread),
            sources,
        })
    }

    /// The followed sources.
    #[must_use]
    pub fn sources(&self) -> &[WatchedSource] {
        &self.sources
    }

    /// Cancels a running rescan and stops the watcher thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.cancel.cancel();
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Changes of one source waiting for their rescan.
#[derive(Debug, Default)]
struct Pending {
    folders: BTreeSet<VfsPath>,
    full: bool,
    first: Option<Instant>,
    last: Option<Instant>,
}

struct Followed {
    source: WatchedSource,
    /// The root as the OS reports it (FSEvents gives resolved paths).
    canonical: Option<PathBuf>,
    pending: Pending,
    next_timer: Option<Instant>,
}

impl Followed {
    fn new(source: WatchedSource, options: &WatchOptions) -> Self {
        let now = Instant::now();
        let next_timer = match source.mode {
            WatchMode::Timer => options.network_interval.map(|i| now + i),
            WatchMode::Events => None,
        };
        let mut pending = Pending::default();
        if options.scan_on_start {
            pending.full = true;
            pending.first = Some(now);
            pending.last = Some(now);
        }
        Self {
            canonical: std::fs::canonicalize(&source.root).ok(),
            source,
            pending,
            next_timer,
        }
    }

    /// Path relative to the root, if `path` is inside it.
    fn relative<'p>(&self, path: &'p Path) -> Option<&'p Path> {
        path.strip_prefix(&self.source.root).ok().or_else(|| {
            self.canonical
                .as_ref()
                .and_then(|c| path.strip_prefix(c).ok())
        })
    }

    fn due(&self, now: Instant, options: &WatchOptions) -> Option<Instant> {
        let p = &self.pending;
        let pending = match (p.first, p.last) {
            (Some(first), Some(last)) if p.full || !p.folders.is_empty() => {
                Some((last + options.debounce).min(first + options.max_delay))
            }
            _ => None,
        };
        let at = match (pending, self.next_timer) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        at.map(|t| t.max(now))
    }
}

struct Run {
    catalog: Arc<Catalog>,
    roots: HashMap<i64, PathBuf>,
    sources: Vec<Followed>,
    options: WatchOptions,
    cancel: CancelToken,
    on_event: WatchCallback,
    /// Kept alive while the thread runs; dropping it ends the OS watch.
    _watcher: Option<RecommendedWatcher>,
}

impl Run {
    fn run(mut self, rx: &Receiver<Msg>) {
        loop {
            let now = Instant::now();
            let next = self
                .sources
                .iter()
                .filter_map(|s| s.due(now, &self.options))
                .min();
            let msg = match next {
                Some(at) => rx.recv_timeout(at.saturating_duration_since(now)),
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match msg {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Ok(Msg::Fs(event)) => self.on_fs(event),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.rescan_due();
            if self.cancel.is_cancelled() {
                return;
            }
        }
    }

    fn on_fs(&mut self, event: notify::Result<notify::Event>) {
        let now = Instant::now();
        let event = match event {
            Ok(e) => e,
            Err(e) => {
                // Lost events (queue overflow, a watch gone): rescan the
                // sources the error names, or all event-watched ones.
                let all = e.paths.is_empty();
                for s in &mut self.sources {
                    if s.source.mode == WatchMode::Events
                        && (all || e.paths.iter().any(|p| s.relative(p).is_some()))
                    {
                        mark(&mut s.pending, None, now);
                    }
                }
                return;
            }
        };
        if !relevant(&event.kind) {
            return;
        }
        let full = event.need_rescan();
        for path in &event.paths {
            for s in &mut self.sources {
                if s.source.mode != WatchMode::Events {
                    continue;
                }
                let Some(rel) = s.relative(path) else {
                    continue;
                };
                if full {
                    mark(&mut s.pending, None, now);
                } else if let Some(folder) = changed_folder(rel) {
                    if hidden(rel) {
                        continue;
                    }
                    if s.pending.folders.len() >= self.options.max_folders {
                        mark(&mut s.pending, None, now);
                    } else {
                        mark(&mut s.pending, Some(folder), now);
                    }
                } else {
                    mark(&mut s.pending, None, now);
                }
            }
        }
    }

    fn rescan_due(&mut self) {
        let now = Instant::now();
        for i in 0..self.sources.len() {
            if self.cancel.is_cancelled() {
                return;
            }
            let s = &mut self.sources[i];
            let p = &s.pending;
            let pending_due = match (p.first, p.last) {
                (Some(first), Some(last)) if p.full || !p.folders.is_empty() => {
                    now >= last + self.options.debounce || now >= first + self.options.max_delay
                }
                _ => false,
            };
            let timer_due = s.next_timer.is_some_and(|t| now >= t);
            if !pending_due && !timer_due {
                continue;
            }
            let pending = std::mem::take(&mut s.pending);
            if let Some(interval) = self.options.network_interval
                && s.source.mode == WatchMode::Timer
            {
                s.next_timer = Some(now + interval);
            }
            let source = s.source.clone();
            let event = if pending.full || timer_due {
                self.full_scan(&source)
            } else {
                self.rescan(&source, pending.folders)
            };
            if !self.cancel.is_cancelled() {
                (self.on_event)(event);
            }
        }
    }

    fn full_scan(&self, source: &WatchedSource) -> WatchEvent {
        let mut event = WatchEvent::new(source.source_id, true);
        let mut errors = Vec::new();
        let result = self.catalog.scan(
            Some(source.source_id),
            &self.roots,
            &self.cancel,
            &mut |u| {
                if let Some(e) = u.error {
                    errors.push(e);
                }
                if u.phase == ScanPhase::Done {
                    event.report.files_seen = u.files_found;
                }
            },
        );
        if let Err(e) = result {
            errors.push(e.to_string());
        }
        if !errors.is_empty() {
            event.error = Some(errors.join("\n"));
        }
        event
    }

    fn rescan(&self, source: &WatchedSource, folders: BTreeSet<VfsPath>) -> WatchEvent {
        let folders = outermost(folders);
        let mut event = WatchEvent::new(source.source_id, false);
        event.folders = folders.iter().map(|f| f.as_str().to_owned()).collect();
        match self
            .catalog
            .rescan_folders(source.source_id, &source.root, &folders, &self.cancel)
        {
            Ok(r) => event.report = r,
            Err(e) => event.error = Some(e.to_string()),
        }
        event
    }
}

/// Adds a change: `folder`, or the whole source for `None`.
fn mark(p: &mut Pending, folder: Option<VfsPath>, now: Instant) {
    match folder {
        Some(f) => {
            p.folders.insert(f);
        }
        None => p.full = true,
    }
    p.first.get_or_insert(now);
    p.last = Some(now);
}

/// Events that can change what the scan sees. Reads (the scan's own
/// fingerprinting, players) and access-time updates are not.
fn relevant(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Metadata(m)) => !matches!(m, MetadataKind::AccessTime),
        _ => true,
    }
}

/// The folder to rescan for a change of `rel` (a path relative to the
/// source root): its parent folder, which also covers a new or removed
/// folder. `None`: the root itself changed, or the path is not valid
/// UTF-8: rescan everything.
fn changed_folder(rel: &Path) -> Option<VfsPath> {
    let mut names = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(n) => names.push(n.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    names.pop()?;
    VfsPath::parse(&names.join("/")).ok()
}

/// A hidden file or folder (`.part` downloads, `.DS_Store`, `.Trash`):
/// the scan skips them anyway.
fn hidden(rel: &Path) -> bool {
    rel.components()
        .any(|c| matches!(c, Component::Normal(n) if n.to_str().is_some_and(|s| s.starts_with('.'))))
}

/// Drops folders inside another folder of the set.
fn outermost(folders: BTreeSet<VfsPath>) -> Vec<VfsPath> {
    let mut out: Vec<VfsPath> = Vec::new();
    // Sorted: a parent comes before its children (`/a` < `/a/b`), but
    // `/a b` sorts between them, so check every kept folder.
    for f in folders {
        if !out.iter().any(|o| within(&f, o)) {
            out.push(f);
        }
    }
    out
}

/// `path` is `dir` or inside it.
fn within(path: &VfsPath, dir: &VfsPath) -> bool {
    dir.is_root()
        || path == dir
        || path
            .as_str()
            .strip_prefix(dir.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(s: &str) -> VfsPath {
        VfsPath::parse(s).unwrap()
    }

    #[test]
    fn a_change_rescans_its_parent_folder() {
        assert_eq!(
            changed_folder(Path::new("Movies/Dune (2021)/Dune.mkv")),
            Some(vp("/Movies/Dune (2021)"))
        );
        assert_eq!(changed_folder(Path::new("Dune.mkv")), Some(VfsPath::root()));
        assert_eq!(changed_folder(Path::new("")), None);
    }

    #[test]
    fn nested_folders_collapse_to_the_outermost() {
        let set: BTreeSet<_> = ["/a", "/a/b", "/a b", "/a b/c", "/c"]
            .into_iter()
            .map(vp)
            .collect();
        assert_eq!(outermost(set), vec![vp("/a"), vp("/a b"), vp("/c")]);
        let with_root: BTreeSet<_> = ["/", "/x"].into_iter().map(vp).collect();
        assert_eq!(outermost(with_root), vec![VfsPath::root()]);
    }

    #[test]
    fn reads_are_not_changes() {
        assert!(!relevant(&EventKind::Access(AccessKind::Read)));
        assert!(relevant(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(!relevant(&EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::AccessTime
        ))));
        assert!(relevant(&EventKind::Create(notify::event::CreateKind::File)));
    }

    #[test]
    fn hidden_paths_are_ignored() {
        assert!(hidden(Path::new("Movies/.Dune.mkv.part")));
        assert!(hidden(Path::new(".Trash/x.mkv")));
        assert!(!hidden(Path::new("Movies/Dune.mkv")));
    }
}
