// SPDX-License-Identifier: MPL-2.0
//! Music indexing: tags of new and changed audio files into the music
//! tables of `pa-library` (MUS-001).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

use pa_library::music::{self, AlbumArt, ArtKind, Orphans, TrackFile, album_folder};
use pa_library::rusqlite::{Connection, OptionalExtension};
use pa_vfs::{Vfs, VfsPath};

use crate::rules::{MediaKind, media_kind};
use crate::scan::{CancelToken, ScanError, ScanEvent, system_now_ms};
use crate::tags::{TagError, read_tags, with_path_fallback};

/// Image names taken as an album cover, in order of preference (lowercase,
/// without extension).
pub const COVER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];

/// Image extensions of a folder cover (lowercase).
pub const COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// How [`index_music`] runs.
#[derive(Debug, Clone)]
pub struct MusicOptions {
    /// Files read in parallel (latency-bound on network sources).
    pub threads: usize,
    /// Tracks written per transaction.
    pub batch_size: usize,
    /// Clock override, Unix milliseconds (tests). `None`: the system clock.
    pub now_ms: Option<i64>,
}

impl Default for MusicOptions {
    fn default() -> Self {
        Self {
            threads: 4,
            batch_size: 200,
            now_ms: None,
        }
    }
}

/// What [`index_music`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MusicReport {
    /// Audio files whose tags were missing or stale.
    pub files: usize,
    /// Files whose tags were read.
    pub read: usize,
    /// Files whose tags could not be read: indexed from their paths.
    pub failed: usize,
    /// Albums recomputed (artists, compilation, year, discs, cover).
    pub albums_refreshed: usize,
    /// Albums, artists and genres removed for having no tracks.
    pub orphans: Orphans,
    /// The run was cancelled; tracks written so far stay.
    pub cancelled: bool,
    pub elapsed: Duration,
}

/// Indexes the music of source `source_id` after a [`scan`](crate::scan):
/// reads the tags of audio files that have no track yet or changed since
/// (in parallel through `vfs`), writes tracks, artists, albums and genres,
/// recomputes the touched albums and their covers and removes orphans.
///
/// A file whose tags cannot be read is still indexed, from its path, and
/// reported as [`ScanEvent::TagsFailed`]; progress comes as
/// [`ScanEvent::TagsProgress`]. A second run without changes reads nothing.
///
/// Blocking; run it on a worker thread. `on_event` is called on the
/// calling thread.
pub fn index_music(
    conn: &mut Connection,
    source_id: i64,
    vfs: &dyn Vfs,
    options: &MusicOptions,
    cancel: &CancelToken,
    on_event: &mut dyn FnMut(&ScanEvent),
) -> Result<MusicReport, ScanError> {
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

    let files: Vec<TrackFile> = music::stale_files(conn, source_id)?
        .into_iter()
        .filter(|f| media_kind(&f.path) == Some(MediaKind::Audio))
        .collect();
    let mut report = MusicReport {
        files: files.len(),
        ..MusicReport::default()
    };
    let mut touched = BTreeSet::new();
    let batch_size = options.batch_size.max(1);

    let threads = options.threads.max(1).min(files.len().max(1));
    let next = AtomicUsize::new(0);
    let stop = Arc::new(AtomicBool::new(false));
    let result = std::thread::scope(|s| -> Result<(), ScanError> {
        let (tx, rx) = sync_channel(threads * 4);
        for _ in 0..threads {
            let tx = tx.clone();
            let (files, next, stop) = (&files, &next, Arc::clone(&stop));
            s.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(file) = files.get(i) else { break };
                    let tags = VfsPath::parse(&file.path)
                        .map_err(TagError::Vfs)
                        .and_then(|p| read_tags(vfs, &p));
                    if tx.send((i, tags)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut batch = Vec::with_capacity(batch_size);
        let outcome = loop {
            if cancel.is_cancelled() {
                report.cancelled = true;
                break Ok(());
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(item) => {
                    batch.push(item);
                    if batch.len() >= batch_size
                        && let Err(e) = write_batch(
                            conn,
                            &files,
                            &mut batch,
                            now,
                            &mut report,
                            &mut touched,
                            on_event,
                        )
                    {
                        break Err(e);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            }
        };
        stop.store(true, Ordering::Relaxed);
        drop(rx);
        outcome?;
        write_batch(
            conn,
            &files,
            &mut batch,
            now,
            &mut report,
            &mut touched,
            on_event,
        )
    });
    result?;

    let tx = conn.transaction()?;
    let mut folders = HashMap::new();
    for album_id in touched {
        if music::refresh_album(&tx, album_id, now)? {
            report.albums_refreshed += 1;
            let art = pick_art(&tx, album_id, source_id, vfs, &mut folders)?;
            music::set_album_art(&tx, album_id, art.as_ref())?;
        }
    }
    report.orphans = music::remove_orphans(&tx)?;
    tx.commit()?;
    report.elapsed = started.elapsed();
    Ok(report)
}

type ReadResult = (usize, Result<pa_library::music::TrackTags, TagError>);

fn write_batch(
    conn: &mut Connection,
    files: &[TrackFile],
    batch: &mut Vec<ReadResult>,
    now: i64,
    report: &mut MusicReport,
    touched: &mut BTreeSet<i64>,
    on_event: &mut dyn FnMut(&ScanEvent),
) -> Result<(), ScanError> {
    if batch.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for (i, result) in batch.drain(..) {
        let file = &files[i];
        let tags = match result {
            Ok(tags) => {
                report.read += 1;
                tags
            }
            Err(e) => {
                report.failed += 1;
                if let Ok(path) = VfsPath::parse(&file.path) {
                    on_event(&ScanEvent::TagsFailed {
                        path,
                        message: e.to_string(),
                    });
                }
                music::TrackTags {
                    tags_failed: true,
                    ..Default::default()
                }
            }
        };
        let put = music::put_track(&tx, file, &with_path_fallback(tags, &file.path), now)?;
        touched.extend(put.album_id);
        touched.extend(put.previous_album_id);
    }
    tx.commit()?;
    on_event(&ScanEvent::TagsProgress {
        done: report.read + report.failed,
        total: report.files,
    });
    Ok(())
}

/// A cover image in the album folder (of this source), else the embedded
/// picture of the first track that has one; a cover from another source is
/// kept when this one has none.
fn pick_art(
    conn: &Connection,
    album_id: i64,
    source_id: i64,
    vfs: &dyn Vfs,
    folders: &mut HashMap<String, Option<String>>,
) -> Result<Option<AlbumArt>, ScanError> {
    let files = music::album_files(conn, album_id)?;
    for f in files.iter().filter(|f| f.source_id == source_id) {
        let dir = album_folder(&f.path).to_owned();
        let cover = folders
            .entry(dir.clone())
            .or_insert_with(|| folder_cover(vfs, &dir));
        if let Some(path) = cover {
            return Ok(Some(AlbumArt {
                kind: ArtKind::Folder,
                source_id,
                path: path.clone(),
            }));
        }
    }
    if let Some(f) = files.iter().find(|f| f.has_embedded_art) {
        return Ok(Some(AlbumArt {
            kind: ArtKind::Embedded,
            source_id: f.source_id,
            path: f.path.clone(),
        }));
    }
    Ok(music::album_art(conn, album_id)?.filter(|a| a.source_id != source_id))
}

/// Path of the preferred cover image in `dir`, if any. A folder that cannot
/// be listed has none.
fn folder_cover(vfs: &dyn Vfs, dir: &str) -> Option<String> {
    let entries = vfs.list(&VfsPath::parse(dir).ok()?).ok()?;
    let mut best: Option<(usize, String)> = None;
    for e in entries.iter().filter(|e| e.metadata.is_file()) {
        let Some((stem, ext)) = e.name.rsplit_once('.') else {
            continue;
        };
        if !COVER_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            continue;
        }
        let stem = stem.to_lowercase();
        if let Some(rank) = COVER_NAMES.iter().position(|n| *n == stem)
            && best.as_ref().is_none_or(|(b, _)| rank < *b)
        {
            best = Some((rank, e.path.as_str().to_owned()));
        }
    }
    best.map(|(_, p)| p)
}
