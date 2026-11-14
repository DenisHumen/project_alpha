// SPDX-License-Identifier: MPL-2.0
//! Structural folder groups (LIB-007): which files of a source form one
//! item and what the other files around it are.
//!
//! `pa-scanner` (`analyze_folders`) computes the groups from a listing of
//! the source; this module stores and reads them:
//!
//! - `file_groups`: one row per group — a movie folder, a series folder, a
//!   disc (BDMV, VIDEO_TS, ISO) or loose files — keyed by its root path;
//! - `file_group_members`: the group's video files that are rows of
//!   `files` (main, parts, extras, samples);
//! - `group_assets`: everything else of the group — sidecar subtitles, NFO
//!   files, local art, extras and samples the shallow scan does not record,
//!   disc streams and ignored leftovers.
//!
//! [`save_groups`] replaces the groups of one source and skips groups whose
//! content did not change, so a rerun on an unchanged source writes nothing.
//!
//! ```
//! use pa_library::groups::{Group, GroupEntry, GroupKind, Role, save_groups, source_groups};
//!
//! let mut conn = pa_library::open_in_memory()?;
//! conn.execute(
//!     "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'M', 'file:///m', 0)",
//!     [],
//! )?;
//! let group = Group {
//!     root: "/Dune (2021)".into(),
//!     kind: GroupKind::MovieFolder,
//!     entries: vec![GroupEntry::new("/Dune (2021)/Dune (2021).mkv", Role::Main)],
//!     ..Group::default()
//! };
//! let report = save_groups(&mut conn, 1, &[group], &[], 0)?;
//! assert_eq!(report.written, 1);
//! assert_eq!(save_groups(&mut conn, 1, &[source_groups(&conn, 1)?[0].group.clone()], &[], 1)?.unchanged, 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::Error;

macro_rules! str_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $var:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($(#[$vm])* $var),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];

            /// The value stored in the database.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }

            /// Parses a stored value.
            #[must_use]
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some($name::$var),)+ _ => None }
            }
        }
    };
}

str_enum!(
    /// What a group is.
    GroupKind {
        /// A folder of one movie (or one item split into parts).
        MovieFolder => "movie_folder",
        /// A folder of several items of a show (a season, a batch).
        SeriesFolder => "series_folder",
        /// A disc structure or image: one item.
        Disc => "disc",
        /// Unrelated files in one folder (a dump folder, the source root).
        Loose => "loose",
    }
);

str_enum!(
    /// Kind of a disc group.
    DiscUnit {
        /// Blu-ray folder structure (`BDMV/`).
        Bdmv => "bdmv",
        /// DVD folder structure (`VIDEO_TS/`).
        VideoTs => "video_ts",
        /// Disc image file (`.iso`).
        Iso => "iso",
    }
);

str_enum!(
    /// Role of a file in its group.
    Role {
        /// The video of an item (a movie, an episode, the main title of a disc).
        Main => "main",
        /// One piece of an item split into files (`cd1`, `part2`, `disc1`,
        /// `VTS_01_2.VOB`); [`GroupEntry::part`] is its number.
        Part => "part",
        /// Trailer, featurette, NCOP… — never an item of its own.
        Extra => "extra",
        /// A sample cut of the release.
        Sample => "sample",
        /// A sidecar subtitle file.
        Subtitle => "subtitle",
        /// An NFO file.
        Nfo => "nfo",
        /// Local artwork (poster, fanart, clearlogo…).
        Art => "art",
        /// A leftover the library should not show (a one-minute "part 3").
        Ignore => "ignore",
    }
);

str_enum!(
    /// Kind of an extra; the values of the corpus guide (`extra_type`).
    ExtraType {
        Trailer => "trailer",
        Teaser => "teaser",
        Featurette => "featurette",
        BehindTheScenes => "behind_the_scenes",
        DeletedScene => "deleted_scene",
        Interview => "interview",
        Short => "short",
        Scene => "scene",
        /// Creditless opening (NCOP).
        Opening => "opening",
        /// Creditless ending (NCED).
        Ending => "ending",
        /// Disc menu.
        Menu => "menu",
        /// Next-episode preview.
        Preview => "preview",
        Other => "other",
    }
);

str_enum!(
    /// Kind of local artwork.
    ArtKind {
        /// `poster.jpg`, `folder.jpg`, `cover.jpg`, `<movie>-poster.jpg`.
        Poster => "poster",
        /// `fanart.jpg`, `backdrop.jpg`, `background.jpg`.
        Fanart => "fanart",
        /// `clearlogo.png`, `logo.png`.
        Clearlogo => "clearlogo",
        /// `clearart.png`.
        Clearart => "clearart",
        /// `banner.jpg`.
        Banner => "banner",
        /// `thumb.jpg`, `landscape.jpg`, `<episode>-thumb.jpg`.
        Thumb => "thumb",
        /// `disc.png`, `discart.png`.
        Discart => "discart",
        /// Booklet and cover scans (`Scans/`).
        Scan => "scan",
    }
);

/// Flags of a sidecar subtitle, read from its name (`movie.ru.forced.srt`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SubtitleInfo {
    /// ISO 639-2 code (`rus`, `eng`), if the name tells.
    pub lang: Option<String>,
    /// Forced subtitles (only foreign-language lines).
    pub forced: bool,
    /// SDH / hearing impaired / closed captions.
    pub hearing_impaired: bool,
    /// Marked as the default track.
    pub default: bool,
}

/// One file of a group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GroupEntry {
    /// Path relative to the source root, as in `files.path`.
    pub path: String,
    pub role: Role,
    /// Part number, 1-based, for [`Role::Part`].
    pub part: Option<u32>,
    /// For [`Role::Extra`].
    pub extra_type: Option<ExtraType>,
    /// The video a sidecar belongs to; `None` for the whole group.
    pub target: Option<String>,
    /// For [`Role::Subtitle`].
    pub subtitle: Option<SubtitleInfo>,
    /// For [`Role::Art`].
    pub art: Option<ArtKind>,
    /// Season of season art (`season01-poster.jpg`; 0 for specials).
    pub season: Option<u32>,
    /// External ids found in an NFO (`tmdb`, `imdb`, `tvdb`…), sorted by key.
    pub ids: BTreeMap<String, String>,
    /// Size in bytes.
    pub size: u64,
    /// The `files` row of this path, if the scan recorded it. Filled by the
    /// readers; ignored by [`save_groups`], which looks it up.
    pub file_id: Option<i64>,
}

impl GroupEntry {
    /// An entry with a path and a role and nothing else.
    #[must_use]
    pub fn new(path: impl Into<String>, role: Role) -> Self {
        Self {
            path: path.into(),
            role,
            part: None,
            extra_type: None,
            target: None,
            subtitle: None,
            art: None,
            season: None,
            ids: BTreeMap::new(),
            size: 0,
            file_id: None,
        }
    }
}

/// A structural group of files.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Group {
    /// The folder of the group (the folder containing `BDMV/` for a disc),
    /// or the image file of an ISO. Unique per source.
    pub root: String,
    pub kind: GroupKind,
    /// For [`GroupKind::Disc`].
    pub disc: Option<DiscUnit>,
    /// Normalized release-name skeleton (03 §4.8), if known.
    pub skeleton: Option<String>,
    /// Files of the group, sorted by path.
    pub entries: Vec<GroupEntry>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            root: String::new(),
            kind: GroupKind::Loose,
            disc: None,
            skeleton: None,
            entries: Vec::new(),
        }
    }
}

impl Group {
    /// The entries that make up items of the library: [`Role::Main`] and
    /// [`Role::Part`], in path order.
    pub fn playable(&self) -> impl Iterator<Item = &GroupEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.role, Role::Main | Role::Part))
    }

    /// The entry with this path.
    #[must_use]
    pub fn entry(&self, path: &str) -> Option<&GroupEntry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

/// A group read from the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGroup {
    pub id: i64,
    pub source_id: i64,
    pub group: Group,
    /// When the group was last written, Unix milliseconds.
    pub analyzed_at_ms: Option<i64>,
}

/// What [`save_groups`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SaveReport {
    /// Groups inserted or rewritten.
    pub written: usize,
    /// Groups whose content did not change (nothing written).
    pub unchanged: usize,
    /// Groups of the source deleted because the analysis no longer has them.
    pub removed: usize,
}

/// Replaces the groups of source `source_id` with `groups`.
///
/// Groups with the same root and the same content are left untouched.
/// Groups not in `groups` are deleted, except those whose root is under one
/// of `keep_under` (folders that failed to list: their groups stay as they
/// were). Each entry is linked to its `files` row by path: video entries
/// with a row go to `file_group_members`, the rest to `group_assets`.
pub fn save_groups(
    conn: &mut Connection,
    source_id: i64,
    groups: &[Group],
    keep_under: &[String],
    now_ms: i64,
) -> Result<SaveReport, Error> {
    let tx = conn.transaction()?;
    let report = save_in(&tx, source_id, groups, keep_under, now_ms)?;
    tx.commit()?;
    Ok(report)
}

fn save_in(
    tx: &Transaction<'_>,
    source_id: i64,
    groups: &[Group],
    keep_under: &[String],
    now_ms: i64,
) -> Result<SaveReport, Error> {
    let mut report = SaveReport::default();
    let mut existing: HashMap<String, (i64, Option<String>)> = HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT root_path, id, analysis_hash FROM file_groups WHERE source_id = ?1",
        )?;
        let rows = stmt.query_map([source_id], |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
        })?;
        for row in rows {
            let (root, v) = row?;
            existing.insert(root, v);
        }
    }

    let mut file_ids = tx.prepare_cached(
        "SELECT id FROM files WHERE source_id = ?1 AND path = ?2 AND missing_since_ms IS NULL",
    )?;
    let mut upsert = tx.prepare_cached(
        "INSERT INTO file_groups (source_id, root_path, kind, skeleton, disc_unit, analysis_hash, analyzed_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (source_id, root_path) DO UPDATE SET
             kind = excluded.kind, skeleton = excluded.skeleton, disc_unit = excluded.disc_unit,
             analysis_hash = excluded.analysis_hash, analyzed_at_ms = excluded.analyzed_at_ms
         RETURNING id",
    )?;
    let mut clear_members = tx.prepare_cached("DELETE FROM file_group_members WHERE group_id = ?1")?;
    let mut clear_assets = tx.prepare_cached("DELETE FROM group_assets WHERE group_id = ?1")?;
    let mut member = tx.prepare_cached(
        "INSERT OR REPLACE INTO file_group_members (group_id, file_id, role, part_no, extra_type)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut asset = tx.prepare_cached(
        "INSERT OR REPLACE INTO group_assets (group_id, path, file_id, role, part_no, extra_type,
             target_path, lang, forced, hearing_impaired, is_default, art_kind, season_number,
             ids_json, size_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )?;

    for group in groups {
        let mut ids = Vec::with_capacity(group.entries.len());
        for e in &group.entries {
            let id: Option<i64> = file_ids
                .query_row(params![source_id, e.path], |r| r.get(0))
                .optional()?;
            ids.push(id);
        }
        let hash = content_hash(group, &ids);
        if let Some((_, Some(old))) = existing.remove(&group.root).filter(|(_, h)| h.is_some())
            && old == hash
        {
            report.unchanged += 1;
            continue;
        }
        let group_id: i64 = upsert.query_row(
            params![
                source_id,
                group.root,
                group.kind.as_str(),
                group.skeleton,
                group.disc.map(DiscUnit::as_str),
                hash,
                now_ms
            ],
            |r| r.get(0),
        )?;
        clear_members.execute([group_id])?;
        clear_assets.execute([group_id])?;
        for (e, file_id) in group.entries.iter().zip(&ids) {
            let part = e.part.map(i64::from);
            let extra = e.extra_type.map(ExtraType::as_str);
            match file_id {
                Some(fid)
                    if matches!(e.role, Role::Main | Role::Part | Role::Extra | Role::Sample) =>
                {
                    member.execute(params![group_id, fid, e.role.as_str(), part, extra])?;
                }
                _ => {
                    let sub = e.subtitle.clone().unwrap_or_default();
                    let ids_json = (!e.ids.is_empty()).then(|| ids_to_json(&e.ids));
                    asset.execute(params![
                        group_id,
                        e.path,
                        file_id,
                        e.role.as_str(),
                        part,
                        extra,
                        e.target,
                        sub.lang,
                        i64::from(sub.forced),
                        i64::from(sub.hearing_impaired),
                        i64::from(sub.default),
                        e.art.map(ArtKind::as_str),
                        e.season.map(i64::from),
                        ids_json,
                        i64::try_from(e.size).unwrap_or(i64::MAX),
                    ])?;
                }
            }
        }
        report.written += 1;
    }

    let mut delete = tx.prepare_cached("DELETE FROM file_groups WHERE id = ?1")?;
    for (root, (id, _)) in existing {
        if keep_under.iter().any(|k| is_under_or_eq(&root, k)) {
            continue;
        }
        delete.execute([id])?;
        report.removed += 1;
    }
    Ok(report)
}

/// Groups of source `source_id`, by root path.
pub fn source_groups(conn: &Connection, source_id: i64) -> Result<Vec<StoredGroup>, Error> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM file_groups WHERE source_id = ?1 ORDER BY root_path")?
        .query_map([source_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    ids.into_iter()
        .map(|id| load_group(conn, id).map(|g| g.expect("group listed above")))
        .collect()
}

/// The group that the `files` row `file_id` belongs to, if any.
pub fn group_of_file(conn: &Connection, file_id: i64) -> Result<Option<StoredGroup>, Error> {
    let id: Option<i64> = conn
        .query_row(
            "SELECT group_id FROM file_group_members WHERE file_id = ?1
             UNION ALL
             SELECT group_id FROM group_assets WHERE file_id = ?1
             LIMIT 1",
            [file_id],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => load_group(conn, id),
        None => Ok(None),
    }
}

/// The group with id `group_id`.
pub fn load_group(conn: &Connection, group_id: i64) -> Result<Option<StoredGroup>, Error> {
    let head = conn
        .query_row(
            "SELECT source_id, root_path, kind, skeleton, disc_unit, analyzed_at_ms
             FROM file_groups WHERE id = ?1",
            [group_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((source_id, root, kind, skeleton, disc, analyzed_at_ms)) = head else {
        return Ok(None);
    };
    let mut entries = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT f.path, m.role, m.part_no, m.extra_type, f.size_bytes, f.id
             FROM file_group_members m JOIN files f ON f.id = m.file_id
             WHERE m.group_id = ?1",
        )?;
        let rows = stmt.query_map([group_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (path, role, part, extra, size, file_id) = row?;
            let mut e = GroupEntry::new(path, parse_role(&role)?);
            e.part = part.and_then(|p| u32::try_from(p).ok());
            e.extra_type = extra.as_deref().and_then(ExtraType::parse);
            e.size = u64::try_from(size).unwrap_or(0);
            e.file_id = Some(file_id);
            entries.push(e);
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT path, role, part_no, extra_type, target_path, lang, forced, hearing_impaired,
                    is_default, art_kind, season_number, ids_json, size_bytes, file_id
             FROM group_assets WHERE group_id = ?1",
        )?;
        let mut rows = stmt.query([group_id])?;
        while let Some(r) = rows.next()? {
            let role = parse_role(&r.get::<_, String>(1)?)?;
            let mut e = GroupEntry::new(r.get::<_, String>(0)?, role);
            e.part = r
                .get::<_, Option<i64>>(2)?
                .and_then(|p| u32::try_from(p).ok());
            e.extra_type = r
                .get::<_, Option<String>>(3)?
                .as_deref()
                .and_then(ExtraType::parse);
            e.target = r.get(4)?;
            if role == Role::Subtitle {
                e.subtitle = Some(SubtitleInfo {
                    lang: r.get(5)?,
                    forced: r.get::<_, i64>(6)? != 0,
                    hearing_impaired: r.get::<_, i64>(7)? != 0,
                    default: r.get::<_, i64>(8)? != 0,
                });
            }
            e.art = r
                .get::<_, Option<String>>(9)?
                .as_deref()
                .and_then(ArtKind::parse);
            e.season = r
                .get::<_, Option<i64>>(10)?
                .and_then(|s| u32::try_from(s).ok());
            if let Some(json) = r.get::<_, Option<String>>(11)? {
                e.ids = ids_from_json(&json);
            }
            e.size = u64::try_from(r.get::<_, i64>(12)?).unwrap_or(0);
            e.file_id = r.get(13)?;
            entries.push(e);
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Some(StoredGroup {
        id: group_id,
        source_id,
        group: Group {
            root,
            kind: GroupKind::parse(&kind).ok_or(Error::Invalid("group kind"))?,
            disc: disc.as_deref().and_then(DiscUnit::parse),
            skeleton,
            entries,
        },
        analyzed_at_ms,
    }))
}

fn parse_role(s: &str) -> Result<Role, Error> {
    Role::parse(s).ok_or(Error::Invalid("group role"))
}

/// FNV-1a 64 of the group as written (with the resolved file ids).
fn content_hash(group: &Group, file_ids: &[Option<i64>]) -> String {
    let mut text = String::new();
    let _ = write!(
        text,
        "{}|{:?}|{:?}",
        group.kind.as_str(),
        group.disc,
        group.skeleton
    );
    for (e, id) in group.entries.iter().zip(file_ids) {
        let _ = write!(
            text,
            "\n{}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{:?}",
            e.path,
            e.role.as_str(),
            e.part,
            e.extra_type,
            e.target,
            e.subtitle,
            e.art,
            e.season,
            e.ids,
            e.size,
            id
        );
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// `{"imdb":"tt0133093","tmdb":"603"}` (keys sorted, strings escaped).
fn ids_to_json(ids: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_str(&mut out, k);
        out.push(':');
        push_json_str(&mut out, v);
    }
    out.push('}');
    out
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Reads what [`ids_to_json`] writes: a flat object of plain strings.
fn ids_from_json(json: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut strings = Vec::new();
    let mut chars = json.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            s.push(c);
                        }
                    }
                    Some(c) => s.push(c),
                    None => {}
                },
                c => s.push(c),
            }
        }
        strings.push(s);
    }
    let mut it = strings.into_iter();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        out.insert(k, v);
    }
    out
}

/// `path` is `dir` or inside it.
fn is_under_or_eq(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip() {
        for v in Role::ALL {
            assert_eq!(Role::parse(v.as_str()), Some(*v));
        }
        for v in ExtraType::ALL {
            assert_eq!(ExtraType::parse(v.as_str()), Some(*v));
        }
        for v in ArtKind::ALL {
            assert_eq!(ArtKind::parse(v.as_str()), Some(*v));
        }
        assert_eq!(GroupKind::parse("disc"), Some(GroupKind::Disc));
        assert_eq!(DiscUnit::parse("video_ts"), Some(DiscUnit::VideoTs));
        assert_eq!(Role::parse("sidecar"), None);
    }

    #[test]
    fn ids_json_round_trip() {
        let mut ids = BTreeMap::new();
        ids.insert("imdb".to_owned(), "tt0133093".to_owned());
        ids.insert("x\"y".to_owned(), "a\\b\n".to_owned());
        let json = ids_to_json(&ids);
        assert_eq!(ids_from_json(&json), ids);
    }

    #[test]
    fn under_or_equal() {
        assert!(is_under_or_eq("/Movies/Dune", "/Movies/Dune"));
        assert!(is_under_or_eq("/Movies/Dune/x", "/Movies"));
        assert!(!is_under_or_eq("/Movies 2/x", "/Movies"));
        assert!(is_under_or_eq("/a", "/"));
    }
}
