// SPDX-License-Identifier: MPL-2.0
//! Import of a `pa_library` v0 database (release 0.1 Dart package), by the
//! mapping of `docs/dev/db.md` §«Переход с pa_library v0».
//!
//! The v0 file is opened read-only and never changed; renaming it to
//! `*.v0.bak` after success is the caller's job. A second run adds nothing:
//! sources are found by URI, files by `(source, path)`, works by title key or
//! TMDB id, a file that already has a live match keeps it, and progress keeps
//! the newer of the stored and the imported state.
//!
//! What moves:
//!
//! - local-folder sources (`file://` URI) and SMB sources (URI as is);
//! - movies and episodes under a local source root: `files`, works (with the
//!   TMDB id as `external_ids`), catalog episodes, matches (`user_set` for
//!   items the user fixed, `auto` otherwise); tracks as `files` (tags are
//!   read by the next scan). Items of SMB sources are left to the next scan:
//!   their mount path is not known here;
//! - progress under `tmdb:` keys for recognized items, otherwise under the
//!   `file:` key of the file, fingerprinted now. Progress of a file that is
//!   gone or unreadable is skipped and counted.
//!
//! Not moved: TMDB cache and settings (the app keeps the TMDB key itself),
//! parse results (files are parsed again when needed).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pa_library::rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use pa_model::MediaKey;

use crate::recognize::{EpisodeNo, WorkIndex, ensure_episode, insert_match};
use crate::{Catalog, Error, fingerprint, sources, uri};

/// What [`Catalog::import_v0`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// v0 sources (found or added).
    pub sources: usize,
    /// v0 items imported as files.
    pub files: usize,
    /// Files that got a match in this run.
    pub matched: usize,
    /// Progress rows written (new or newer).
    pub progress: usize,
    /// Progress rows without a key (file gone or on an SMB source).
    pub progress_skipped: usize,
    /// Items outside any local source root (SMB, or moved).
    pub items_skipped: usize,
}

struct V0Item {
    id: String,
    kind: String,
    source_id: Option<String>,
    title: String,
    original_title: Option<String>,
    year: Option<i64>,
    overview: Option<String>,
    show_id: Option<String>,
    season: Option<i64>,
    episode: Option<i64>,
    episode_end: Option<i64>,
    file_path: String,
    file_size: Option<i64>,
    file_mtime: Option<i64>,
    duration_ms: Option<i64>,
    match_confidence: Option<f64>,
    tmdb_id: Option<i64>,
    locked: bool,
    added_at: i64,
}

fn load_items(v0: &Connection) -> Result<Vec<V0Item>, Error> {
    let mut stmt = v0.prepare(
        "SELECT id, kind, source_id, title, original_title, year, overview, show_id, season,
                episode, episode_end, file_path, file_size, file_mtime, duration_ms,
                match_confidence, tmdb_id, locked, added_at
         FROM items ORDER BY CASE kind WHEN 'show' THEN 0 ELSE 1 END, file_path",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(V0Item {
            id: r.get(0)?,
            kind: r.get(1)?,
            source_id: r.get(2)?,
            title: r.get(3)?,
            original_title: r.get(4)?,
            year: r.get(5)?,
            overview: r.get(6)?,
            show_id: r.get(7)?,
            season: r.get(8)?,
            episode: r.get(9)?,
            episode_end: r.get(10)?,
            file_path: r.get(11)?,
            file_size: r.get(12)?,
            file_mtime: r.get(13)?,
            duration_ms: r.get(14)?,
            match_confidence: r.get(15)?,
            tmdb_id: r.get(16)?,
            locked: r.get::<_, Option<i64>>(17)?.unwrap_or(0) != 0,
            added_at: r.get(18)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Work with this TMDB id, or the local one by title key; the TMDB id and
/// v0 metadata are attached.
fn ensure_work(
    tx: &Transaction<'_>,
    index: &mut WorkIndex,
    kind: &str,
    item: &V0Item,
    now: i64,
) -> Result<i64, Error> {
    let ext_kind = if kind == "movie" { "movie" } else { "tv" };
    if let Some(tmdb) = item.tmdb_id {
        let found: Option<i64> = tx
            .query_row(
                "SELECT work_id FROM external_ids WHERE provider = 'tmdb' AND ext_kind = ?1 AND ext_id = ?2",
                params![ext_kind, tmdb.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(w) = found {
            return Ok(w);
        }
    }
    let work = index.ensure(tx, kind, &item.title, item.year, now)?;
    tx.execute(
        "UPDATE works SET original_title = ifnull(original_title, ?2),
                          overview = ifnull(overview, ?3),
                          year = ifnull(year, ?4)
         WHERE id = ?1",
        params![
            work,
            item.original_title,
            item.overview,
            item.year.filter(|y| (1800..=3000).contains(y))
        ],
    )?;
    if let Some(tmdb) = item.tmdb_id {
        tx.execute(
            "INSERT OR IGNORE INTO external_ids (work_id, provider, ext_kind, ext_id)
             VALUES (?1, 'tmdb', ?2, ?3)",
            params![work, ext_kind, tmdb.to_string()],
        )?;
    }
    Ok(work)
}

/// `files` row of a v0 item (inserted if new); `None` if the item is not
/// under the root of its source.
fn ensure_file(
    tx: &Transaction<'_>,
    source: i64,
    root: &Path,
    item: &V0Item,
) -> Result<Option<i64>, Error> {
    let Ok(rel) = Path::new(&item.file_path).strip_prefix(root) else {
        return Ok(None);
    };
    // Stored as a `VfsPath`, like the scanner does: `/Movies/x.mkv`.
    let rel = format!("/{}", rel.to_string_lossy().replace('\\', "/"));
    if rel == "/" {
        return Ok(None);
    }
    tx.execute(
        "INSERT INTO files (source_id, path, size_bytes, mtime_ms, duration_ms, added_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (source_id, path) DO NOTHING",
        params![
            source,
            rel,
            item.file_size.unwrap_or(0).max(0),
            item.file_mtime,
            item.duration_ms.filter(|d| *d >= 0),
            item.added_at
        ],
    )?;
    Ok(Some(tx.query_row(
        "SELECT id FROM files WHERE source_id = ?1 AND path = ?2",
        params![source, rel],
        |r| r.get(0),
    )?))
}

/// Key of an imported v0 item for its progress.
enum KeyPlan {
    Key(MediaKey, &'static str),
    /// Fingerprint this local file and use its `file:` key.
    File(i64, PathBuf, &'static str),
    None,
}

impl Catalog {
    /// Imports the `pa_library` v0 database at `v0_path` (read-only).
    pub fn import_v0(&self, v0_path: &Path) -> Result<ImportReport, Error> {
        let v0 = Connection::open_with_flags(
            v0_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        if !pa_library::is_legacy_v0(&v0)? {
            return Err(Error::Invalid(format!(
                "not a pa_library v0 database: {}",
                v0_path.display()
            )));
        }
        let mut report = ImportReport::default();
        let now = crate::now_ms();
        let mut conn = self.lock();
        let tx = conn.transaction()?;

        // Sources.
        let mut src_map: HashMap<String, (i64, Option<PathBuf>)> = HashMap::new();
        {
            let mut stmt = v0.prepare(
                "SELECT id, kind, name, path, username FROM sources ORDER BY position, name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?;
            for row in rows {
                let (id, kind, name, path, username) = row?;
                let (kind, uri, root) = if kind == "smb" {
                    ("smb", path.clone(), None)
                } else {
                    let root = PathBuf::from(&path);
                    ("local", uri::path_to_file_uri(&root), Some(root))
                };
                let new_id = match sources::by_uri(&tx, &uri)? {
                    Some(existing) => existing,
                    None => sources::insert(&tx, kind, &name, &uri, username.as_deref(), now)?,
                };
                src_map.insert(id, (new_id, root));
                report.sources += 1;
            }
        }

        // Items.
        let items = load_items(&v0)?;
        let mut index = WorkIndex::load(&tx)?;
        let mut show_works: HashMap<String, i64> = HashMap::new();
        let mut keys: HashMap<String, KeyPlan> = HashMap::new();
        let shows: HashMap<&str, &V0Item> = items
            .iter()
            .filter(|i| i.kind == "show")
            .map(|i| (i.id.as_str(), i))
            .collect();
        for item in &items {
            if item.kind == "show" {
                continue;
            }
            let Some((source, Some(root))) = item.source_id.as_ref().and_then(|s| src_map.get(s))
            else {
                report.items_skipped += 1;
                continue;
            };
            let Some(file) = ensure_file(&tx, *source, root, item)? else {
                report.items_skipped += 1;
                continue;
            };
            report.files += 1;
            let (state, decided_by, conf) = if item.locked {
                ("user_set", "user", 1.0)
            } else {
                ("auto", "scorer", item.match_confidence.unwrap_or(0.5))
            };
            let abs = PathBuf::from(&item.file_path);
            match item.kind.as_str() {
                "movie" => {
                    let work = ensure_work(&tx, &mut index, "movie", item, now)?;
                    if insert_match(&tx, file, work, &[], conf, state, decided_by, None, now)? {
                        report.matched += 1;
                    }
                    let plan = match item.tmdb_id {
                        Some(id) => KeyPlan::Key(
                            MediaKey::TmdbMovie {
                                id: id.unsigned_abs(),
                            },
                            "movie",
                        ),
                        None => KeyPlan::File(file, abs, "movie"),
                    };
                    keys.insert(item.id.clone(), plan);
                }
                "episode" => {
                    let show = item.show_id.as_deref().and_then(|s| shows.get(s).copied());
                    let work = match (show, item.show_id.as_ref()) {
                        (Some(show), Some(sid)) => match show_works.get(sid) {
                            Some(w) => *w,
                            None => {
                                let w = ensure_work(&tx, &mut index, "show", show, now)?;
                                show_works.insert(sid.clone(), w);
                                w
                            }
                        },
                        _ => {
                            report.items_skipped += 1;
                            continue;
                        }
                    };
                    let season = item.season.unwrap_or(1);
                    let first = item.episode.unwrap_or(1);
                    let last = item.episode_end.filter(|e| *e >= first).unwrap_or(first);
                    let mut eps = Vec::new();
                    for number in first..=last.min(first + 50) {
                        let no = EpisodeNo {
                            season: Some(season),
                            episode: Some(number),
                            absolute: None,
                            kind: "regular",
                        };
                        let title = (first == last).then_some(item.title.as_str());
                        eps.push(ensure_episode(&tx, work, no, title)?);
                    }
                    if insert_match(&tx, file, work, &eps, conf, state, decided_by, None, now)? {
                        report.matched += 1;
                    }
                    let show_tmdb = show.and_then(|s| s.tmdb_id);
                    let plan = match show_tmdb {
                        Some(id) => KeyPlan::Key(
                            MediaKey::TmdbEpisode {
                                show_id: id.unsigned_abs(),
                                season: u32::try_from(season).unwrap_or(0),
                                episode: u32::try_from(last).unwrap_or(0),
                            },
                            "episode",
                        ),
                        None => KeyPlan::File(file, abs, "episode"),
                    };
                    keys.insert(item.id.clone(), plan);
                }
                "track" => {
                    keys.insert(item.id.clone(), KeyPlan::File(file, abs, "track"));
                }
                _ => {}
            }
        }

        // Progress.
        let mut stmt = v0.prepare(
            "SELECT item_id, position_ms, duration_ms, updated_at, watched FROM progress",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            let (item_id, position, duration, updated, watched) = row?;
            let (key, kind) = match keys.get(&item_id).unwrap_or(&KeyPlan::None) {
                KeyPlan::Key(k, kind) => (k.clone(), *kind),
                KeyPlan::File(file, path, kind) => {
                    let existing: Option<String> =
                        tx.query_row("SELECT fingerprint FROM files WHERE id = ?1", [file], |r| {
                            r.get(0)
                        })?;
                    let fp = match existing {
                        Some(fp) => fp,
                        None => match fingerprint::oshash_file(path) {
                            Ok(fp) => {
                                tx.execute(
                                    "UPDATE files SET fingerprint = ?2, oshash = ?2, hashed_at_ms = ?3
                                     WHERE id = ?1",
                                    params![file, fp, now],
                                )?;
                                fp
                            }
                            Err(_) => {
                                report.progress_skipped += 1;
                                continue;
                            }
                        },
                    };
                    (MediaKey::File { fingerprint: fp }, *kind)
                }
                KeyPlan::None => {
                    report.progress_skipped += 1;
                    continue;
                }
            };
            let written = tx.execute(
                "INSERT INTO progress (profile_id, media_key, source_type, position_ms, duration_ms,
                                       watched, updated_at_ms, device_id)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, 'import-v0')
                 ON CONFLICT (profile_id, media_key) DO UPDATE SET
                     position_ms = excluded.position_ms,
                     duration_ms = ifnull(excluded.duration_ms, progress.duration_ms),
                     watched = max(progress.watched, excluded.watched),
                     updated_at_ms = excluded.updated_at_ms
                 WHERE excluded.updated_at_ms > progress.updated_at_ms",
                params![
                    key.to_string(),
                    kind,
                    position.max(0),
                    Some(duration).filter(|d| *d > 0),
                    i64::from(watched != 0),
                    updated
                ],
            )?;
            report.progress += written;
        }
        drop(stmt);
        tx.commit()?;
        pa_library::search::sync(&conn)?;
        Ok(report)
    }
}
