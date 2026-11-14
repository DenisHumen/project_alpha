// SPDX-License-Identifier: MPL-2.0
//! Library search (LIB-008, docs/research/01 §1.5, 06 §5.3): SQLite FTS5
//! over movies, shows, anime, episodes, unrecognised videos, music artists,
//! albums and tracks and collections; as-you-type prefix matching, ranking
//! and grouping. People and Twitch (VODs, streamers) join when their tables
//! exist (`TWI-*`): a new [`SearchKind`] and a document builder.
//!
//! **Normalization.** Every searchable string (a title, an alias from
//! `titles`, a file name) is split into tokens by
//! [`pa_recognition::title::normalize`], the same keys the recognizer uses:
//!
//! - *plain* — NFKC, case folding, Latin diacritics removed (`Amélie` →
//!   `amelie`, `ё` → `е`), punctuation dropped, roman numerals arabic;
//! - *loose* — Cyrillic transliterated into one Latin space where common
//!   spellings collapse (`Ирония`, `Ironiya`, `Ironija`, `Ironia` →
//!   `ironya`), plus the whole title without spaces (`spiderman`).
//!
//! A query matches when every token is found in either key, the last one as
//! a prefix (as-you-type), so `ирония судьбы` finds the file
//! `Ironiya.sudby.1975.mkv` and `ведьмак` finds *The Witcher* through its
//! Russian alias. Titles in other languages come from `titles`; search does
//! not translate.
//!
//! **Ranking.** Candidates from FTS5 (bm25) and from the exact and
//! starts-with lookups of the keys are scored in Rust: exact title >
//! title starts with the query > all tokens anywhere; then how much of the
//! title the query covers, the weight of the alias (main title first) and of
//! the kind (works before episodes and files). One hit per entity.
//!
//! **Keeping the index fresh.** Keys are computed in Rust, so SQL triggers
//! only queue changed entities in `search_queue`; [`sync`] rebuilds their
//! documents. [`search`] calls it first when the queue is not empty, so
//! writers need not know about search; a scanner may call [`sync`] after a
//! batch to keep the first query fast.
//!
//! ```
//! use pa_library::search::{self, SearchKind, SearchOptions};
//!
//! let conn = pa_library::open_in_memory().unwrap();
//! conn.execute_batch(
//!     "INSERT INTO works (id, kind, title, year, created_at_ms, updated_at_ms)
//!          VALUES (1, 'show', 'The Witcher', 2019, 0, 0);
//!      INSERT INTO titles (work_id, lang, text, kind) VALUES (1, 'ru', 'Ведьмак', 'official');",
//! )
//! .unwrap();
//! let hits = search::search(&conn, "ведьм", &SearchOptions::default()).unwrap();
//! assert_eq!((hits[0].kind, hits[0].id), (SearchKind::Show, 1));
//! assert_eq!(hits[0].title, "The Witcher");
//! assert_eq!(hits[0].matched, "Ведьмак");
//! ```

use std::collections::HashMap;
use std::fmt::Write as _;

use pa_recognition::title::TitleKey;
use pa_recognition::title::normalize::{fold, loose_token};
use pa_recognition::title::translit::{Scheme, to_latin};
use rusqlite::{Connection, OptionalExtension, params};

use crate::Error;

/// Default number of hits.
pub const DEFAULT_LIMIT: usize = 50;
/// Largest accepted [`SearchOptions::limit`].
pub const MAX_LIMIT: usize = 500;

/// Entities processed per [`sync`] savepoint.
const SYNC_BATCH: i64 = 500;
/// FTS5 candidates per query (before ranking and deduplication).
const FTS_CANDIDATES: usize = 300;
/// Candidates of each exact / starts-with key lookup.
const KEY_CANDIDATES: usize = 100;

/// File extensions of video files indexed while unrecognised.
const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "avi", "divx", "flv", "iso", "m2ts", "m4v", "mk3d", "mkv", "mov", "mp4", "mpeg", "mpg",
    "mts", "ogm", "ogv", "rm", "rmvb", "ts", "vob", "webm", "wmv",
];

/// Result group of a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SearchKind {
    /// `works.kind = 'movie'`; [`SearchHit::id`] is `works.id`.
    Movie,
    /// `works.kind = 'show'`; `works.id`.
    Show,
    /// `works.kind = 'anime'`; `works.id`.
    Anime,
    /// `episodes.id`.
    Episode,
    /// A video file without a recognised work; `files.id`.
    Video,
    /// `music_artists.id`.
    Artist,
    /// `music_albums.id`.
    Album,
    /// `music_tracks.id`.
    Track,
    /// `collections.id`.
    Collection,
}

impl SearchKind {
    /// Every kind, in the default group order.
    pub const ALL: [SearchKind; 9] = [
        Self::Movie,
        Self::Show,
        Self::Anime,
        Self::Collection,
        Self::Artist,
        Self::Album,
        Self::Track,
        Self::Episode,
        Self::Video,
    ];

    /// Value of `search_docs.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Show => "show",
            Self::Anime => "anime",
            Self::Episode => "episode",
            Self::Video => "video",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Track => "track",
            Self::Collection => "collection",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Ranking bonus of the kind: works first, then collections and music,
    /// episodes and unrecognised files last.
    fn weight(self) -> f64 {
        match self {
            Self::Movie | Self::Show | Self::Anime => 1.0,
            Self::Artist => 0.9,
            Self::Collection => 0.8,
            Self::Album => 0.7,
            Self::Video => 0.6,
            Self::Track => 0.5,
            Self::Episode => 0.4,
        }
    }
}

/// Parameters of [`search`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    /// Maximum number of hits (1..=[`MAX_LIMIT`]).
    pub limit: usize,
    /// Kinds to search; empty means all.
    pub kinds: Vec<SearchKind>,
    /// Profile of the viewer: other profiles' collections are left out.
    /// `None` shows every collection.
    pub profile_id: Option<i64>,
    /// At most this many hits of each kind (grouped results, UI-011): the
    /// best hits of a crowded kind do not push the others out of `limit`.
    /// One query ranks every kind, which is cheaper than a query per kind.
    pub per_kind: Option<usize>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            kinds: Vec::new(),
            profile_id: None,
            per_kind: None,
        }
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub kind: SearchKind,
    /// Row id in the table of the kind (see [`SearchKind`]).
    pub id: i64,
    /// Display title: the work's title, the episode title (or `S01E02`),
    /// the file name, the artist, album, track or collection name.
    pub title: String,
    /// Context: the show and episode number, the album artists, the track's
    /// artist and album, the file path.
    pub subtitle: Option<String>,
    pub year: Option<i64>,
    /// The string that matched (an alias such as `Ведьмак` when the title is
    /// `The Witcher`).
    pub matched: String,
    /// Relevance; higher is better. Comparable within one query only.
    pub score: f64,
}

/// Hits of one kind, see [`group`].
#[derive(Debug, Clone, PartialEq)]
pub struct SearchGroup {
    pub kind: SearchKind,
    pub hits: Vec<SearchHit>,
}

/// Groups hits by kind. Groups are ordered by their best hit, hits keep
/// their order.
pub fn group(hits: Vec<SearchHit>) -> Vec<SearchGroup> {
    let mut groups: Vec<SearchGroup> = Vec::new();
    for hit in hits {
        match groups.iter_mut().find(|g| g.kind == hit.kind) {
            Some(g) => g.hits.push(hit),
            None => groups.push(SearchGroup {
                kind: hit.kind,
                hits: vec![hit],
            }),
        }
    }
    groups
}

// --- Keys --------------------------------------------------------------------------

/// Plain and loose form of one folded token.
fn token_keys(tok: &str) -> (String, String) {
    let k = TitleKey::new(tok);
    let loose = if k.loose.is_empty() {
        k.plain.clone()
    } else {
        k.loose
    };
    (k.plain, loose)
}

/// Search keys of one string.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DocKeys {
    plain: Vec<String>,
    loose: Vec<String>,
}

impl DocKeys {
    fn new(text: &str) -> Option<Self> {
        let folded = fold(text);
        if folded.is_empty() {
            return None;
        }
        let (plain, loose) = folded.split(' ').map(token_keys).unzip();
        Some(Self { plain, loose })
    }

    /// Text of the FTS `loose` column: the tokens and, for several tokens,
    /// the title without spaces.
    fn loose_column(&self) -> String {
        let mut s = self.loose.join(" ");
        if self.loose.len() > 1 {
            s.push(' ');
            s.push_str(&self.loose.concat());
        }
        s
    }
}

/// A parsed query: full keys of every token and the prefix alternatives of
/// the last one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Query {
    plain: Vec<String>,
    loose: Vec<String>,
    /// Prefixes of the last token in the plain key: as typed, and with
    /// numerals converted.
    plain_prefixes: Vec<String>,
    /// Prefixes of the last token in the loose key: the full loose form, and
    /// a shortened transliteration for a word typed halfway in the other
    /// script (`ирони` → `iron`, which `ironya` starts with).
    loose_prefixes: Vec<String>,
}

impl Query {
    fn parse(q: &str) -> Option<Self> {
        let folded = fold(q);
        if folded.is_empty() {
            return None;
        }
        let raw: Vec<&str> = folded.split(' ').collect();
        let (plain, loose): (Vec<String>, Vec<String>) = raw.iter().map(|t| token_keys(t)).unzip();
        let last_raw = *raw.last()?;
        let mut plain_prefixes = vec![last_raw.to_owned()];
        push_unique(&mut plain_prefixes, plain.last()?.clone());
        let mut loose_prefixes = vec![loose.last()?.clone()];
        if let Some(p) = partial_loose(last_raw) {
            push_unique(&mut loose_prefixes, p);
        }
        Some(Self {
            plain,
            loose,
            plain_prefixes,
            loose_prefixes,
        })
    }

    fn n(&self) -> usize {
        self.plain.len()
    }

    /// FTS5 MATCH expression: every token in either column, the last one as
    /// a prefix.
    fn fts_expr(&self) -> String {
        let n = self.n();
        let mut parts = Vec::with_capacity(n);
        for (p, l) in self.plain[..n - 1].iter().zip(&self.loose[..n - 1]) {
            parts.push(format!("(plain:{} OR loose:{})", quote(p), quote(l)));
        }
        let mut last = String::from("(");
        for (i, p) in self.plain_prefixes.iter().enumerate() {
            if i > 0 {
                last.push_str(" OR ");
            }
            let _ = write!(last, "plain:{}*", quote(p));
        }
        for p in &self.loose_prefixes {
            let _ = write!(last, " OR loose:{}*", quote(p));
        }
        last.push(')');
        parts.push(last);
        parts.join(" AND ")
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !s.is_empty() && !v.contains(&s) {
        v.push(s);
    }
}

/// FTS5 string literal.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Loose prefix of a token typed halfway: transliterated, without the
/// trailing letters that may still become part of a digraph or an ending
/// (`ironi` of `ironiya`, `kh` of `khrustalev`). `None` if nothing useful is
/// left or it equals the full form.
fn partial_loose(raw: &str) -> Option<String> {
    let mut lat = to_latin(raw, Scheme::Common);
    if !lat.is_ascii() {
        return None;
    }
    let mut stripped = 0;
    while stripped < 2
        && lat.len() > 3
        && lat
            .as_bytes()
            .last()
            .is_some_and(|b| b"aeiouyjhkzsct".contains(b))
    {
        lat.pop();
        stripped += 1;
    }
    let p = loose_token(&lat);
    (p.len() >= 2).then_some(p)
}

// --- Index maintenance ---------------------------------------------------------------

/// True if some entities wait for [`sync`].
pub fn pending(conn: &Connection) -> Result<bool, Error> {
    Ok(
        conn.query_row("SELECT EXISTS (SELECT 1 FROM search_queue)", [], |r| {
            r.get(0)
        })?,
    )
}

/// Rebuilds the documents of queued entities; returns how many entities were
/// processed. Runs in savepoints of a few hundred entities, so it works
/// inside or outside a caller's transaction.
pub fn sync(conn: &Connection) -> Result<usize, Error> {
    let mut total = 0;
    loop {
        let batch: Vec<(String, i64)> = conn
            .prepare_cached("SELECT entity, ref_id FROM search_queue LIMIT ?1")?
            .query_map([SYNC_BATCH], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        if batch.is_empty() {
            return Ok(total);
        }
        savepoint(conn, |conn| {
            for (entity, id) in &batch {
                reindex(conn, entity, *id)?;
                conn.prepare_cached("DELETE FROM search_queue WHERE entity = ?1 AND ref_id = ?2")?
                    .execute(params![entity, id])?;
            }
            Ok(())
        })?;
        total += batch.len();
    }
}

/// Queues every entity and rebuilds the whole index (repair, or after a
/// change of the normalization rules).
pub fn rebuild(conn: &Connection) -> Result<usize, Error> {
    savepoint(conn, |conn| {
        conn.execute_batch(
            "INSERT INTO search_fts (search_fts) VALUES ('delete-all');
             DELETE FROM search_docs;
             INSERT OR IGNORE INTO search_queue
                 SELECT 'work', id FROM works
                 UNION ALL SELECT 'episode', id FROM episodes
                 UNION ALL SELECT 'file', id FROM files
                 UNION ALL SELECT 'artist', id FROM music_artists
                 UNION ALL SELECT 'album', id FROM music_albums
                 UNION ALL SELECT 'track', id FROM music_tracks
                 UNION ALL SELECT 'collection', id FROM collections;",
        )?;
        Ok(())
    })?;
    sync(conn)
}

fn savepoint<T>(
    conn: &Connection,
    f: impl FnOnce(&Connection) -> Result<T, Error>,
) -> Result<T, Error> {
    conn.execute_batch("SAVEPOINT pa_search")?;
    match f(conn) {
        Ok(v) => {
            conn.execute_batch("RELEASE pa_search")?;
            Ok(v)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK TO pa_search; RELEASE pa_search");
            Err(e)
        }
    }
}

/// Replaces the documents of one entity.
fn reindex(conn: &Connection, entity: &str, id: i64) -> Result<(), Error> {
    conn.prepare_cached(
        "DELETE FROM search_fts WHERE rowid IN \
         (SELECT id FROM search_docs WHERE entity = ?1 AND ref_id = ?2)",
    )?
    .execute(params![entity, id])?;
    conn.prepare_cached("DELETE FROM search_docs WHERE entity = ?1 AND ref_id = ?2")?
        .execute(params![entity, id])?;

    let Some((kind, strings, extra)) = documents(conn, entity, id)? else {
        return Ok(());
    };
    // One document per distinct key; the heaviest string wins.
    let mut docs: Vec<(String, DocKeys, f64)> = Vec::new();
    for (text, weight) in strings {
        let Some(keys) = DocKeys::new(&text) else {
            continue;
        };
        match docs.iter_mut().find(|(_, k, _)| k.plain == keys.plain) {
            Some(d) if d.2 >= weight => {}
            Some(d) => *d = (text, keys, weight),
            None => docs.push((text, keys, weight)),
        }
    }
    for (text, keys, weight) in docs {
        let plain_key = keys.plain.join(" ");
        let loose_key = keys.loose.join(" ");
        conn.prepare_cached(
            "INSERT INTO search_docs (entity, ref_id, kind, text, plain_key, loose_key, weight) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            entity,
            id,
            kind.as_str(),
            text,
            plain_key,
            loose_key,
            weight
        ])?;
        let rowid = conn.last_insert_rowid();
        let mut plain_col = plain_key;
        if let Some(extra) = &extra {
            plain_col.push(' ');
            plain_col.push_str(extra);
        }
        conn.prepare_cached("INSERT INTO search_fts (rowid, plain, loose) VALUES (?1, ?2, ?3)")?
            .execute(params![rowid, plain_col, keys.loose_column()])?;
    }
    Ok(())
}

/// Kind, searchable strings with weights and extra plain tokens (a work's
/// year) of an entity; `None` if it does not exist or is not searchable.
type Documents = (SearchKind, Vec<(String, f64)>, Option<String>);

fn documents(conn: &Connection, entity: &str, id: i64) -> Result<Option<Documents>, Error> {
    Ok(match entity {
        "work" => work_documents(conn, id)?,
        "episode" => conn
            .prepare_cached("SELECT title FROM episodes WHERE id = ?1 AND title IS NOT NULL")?
            .query_row([id], |r| r.get::<_, String>(0))
            .optional()?
            .map(|t| (SearchKind::Episode, vec![(t, 1.0)], None)),
        "file" => file_documents(conn, id)?,
        "artist" => conn
            .prepare_cached("SELECT name, sort_name FROM music_artists WHERE id = ?1")?
            .query_row([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .optional()?
            .map(|(name, sort)| {
                let mut v = vec![(name, 1.0)];
                v.extend(sort.map(|s| (s, 0.9)));
                (SearchKind::Artist, v, None)
            }),
        "album" => {
            let title: Option<String> = conn
                .prepare_cached("SELECT title FROM music_albums WHERE id = ?1")?
                .query_row([id], |r| r.get(0))
                .optional()?;
            match title {
                None => None,
                Some(title) => {
                    let artists: Vec<String> = conn
                        .prepare_cached(
                            "SELECT a.name FROM music_album_artists aa \
                             JOIN music_artists a ON a.id = aa.artist_id \
                             WHERE aa.album_id = ?1 ORDER BY aa.position",
                        )?
                        .query_map([id], |r| r.get(0))?
                        .collect::<Result<_, _>>()?;
                    let mut v: Vec<(String, f64)> = artists
                        .iter()
                        .map(|a| (format!("{a} {title}"), 0.8))
                        .collect();
                    v.insert(0, (title, 1.0));
                    Some((SearchKind::Album, v, None))
                }
            }
        }
        "track" => {
            let row: Option<(String, Option<String>)> = conn
                .prepare_cached(
                    "SELECT t.title, (SELECT a.name FROM music_track_artists ta \
                                      JOIN music_artists a ON a.id = ta.artist_id \
                                      WHERE ta.track_id = t.id AND ta.role = 'artist' \
                                      ORDER BY ta.position LIMIT 1) \
                     FROM music_tracks t WHERE t.id = ?1",
                )?
                .query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            row.map(|(title, artist)| {
                let mut v = Vec::new();
                if let Some(a) = artist {
                    v.push((format!("{a} {title}"), 0.8));
                }
                v.insert(0, (title, 1.0));
                (SearchKind::Track, v, None)
            })
        }
        "collection" => conn
            .prepare_cached("SELECT name FROM collections WHERE id = ?1")?
            .query_row([id], |r| r.get::<_, String>(0))
            .optional()?
            .map(|n| (SearchKind::Collection, vec![(n, 1.0)], None)),
        _ => None,
    })
}

fn work_documents(conn: &Connection, id: i64) -> Result<Option<Documents>, Error> {
    let row: Option<(String, String, Option<String>, Option<i64>)> = conn
        .prepare_cached("SELECT kind, title, original_title, year FROM works WHERE id = ?1")?
        .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .optional()?;
    let Some((kind, title, original, year)) = row else {
        return Ok(None);
    };
    let Some(kind) = SearchKind::parse(&kind) else {
        return Ok(None);
    };
    let mut strings = vec![(title, 1.0)];
    strings.extend(original.map(|t| (t, 0.95)));
    let mut stmt = conn.prepare_cached("SELECT text, kind FROM titles WHERE work_id = ?1")?;
    let titles = stmt.query_map([id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for t in titles {
        let (text, tkind) = t?;
        let weight = match tkind.as_str() {
            "main" => 1.0,
            "official" => 0.95,
            "romaji" => 0.9,
            "synonym" => 0.85,
            _ => 0.8,
        };
        strings.push((text, weight));
    }
    Ok(Some((kind, strings, year.map(|y| y.to_string()))))
}

fn file_documents(conn: &Connection, id: i64) -> Result<Option<Documents>, Error> {
    let row: Option<String> = conn
        .prepare_cached(
            "SELECT f.path FROM files f WHERE f.id = ?1 AND f.missing_since_ms IS NULL \
             AND NOT EXISTS (SELECT 1 FROM matches m WHERE m.file_id = f.id AND m.state <> 'rejected') \
             AND NOT EXISTS (SELECT 1 FROM music_tracks t WHERE t.file_id = f.id)",
        )?
        .query_row([id], |r| r.get(0))
        .optional()?;
    let Some(path) = row else {
        return Ok(None);
    };
    let (dir, name) = split_path(&path);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return Ok(None);
    };
    if !VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
        return Ok(None);
    }
    let mut v = vec![(stem.to_owned(), 1.0)];
    if let Some(dir) = dir.rsplit('/').next().filter(|d| !d.is_empty()) {
        v.push((dir.to_owned(), 0.7));
    }
    Ok(Some((SearchKind::Video, v, None)))
}

/// `(parent directory, file name)` of a POSIX path.
fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((d, n)) => (d, n),
        None => ("", path),
    }
}

// --- Query ---------------------------------------------------------------------------

/// A candidate document.
struct Candidate {
    entity: String,
    ref_id: i64,
    kind: SearchKind,
    text: String,
    plain_key: String,
    loose_key: String,
    weight: f64,
    /// bm25 of the FTS match (lower is better); 0 for key lookups.
    bm25: f64,
}

/// Searches the library. Brings the index up to date first ([`sync`]) if
/// changes are queued. An empty query returns nothing.
pub fn search(
    conn: &Connection,
    query: &str,
    opts: &SearchOptions,
) -> Result<Vec<SearchHit>, Error> {
    if opts.limit == 0 || opts.limit > MAX_LIMIT {
        return Err(Error::Invalid("search limit"));
    }
    if pending(conn)? {
        sync(conn)?;
    }
    let Some(q) = Query::parse(query) else {
        return Ok(Vec::new());
    };
    let kinds: Vec<SearchKind> = if opts.kinds.is_empty() {
        SearchKind::ALL.to_vec()
    } else {
        opts.kinds.clone()
    };
    let kind_list = kinds
        .iter()
        .map(|k| format!("'{}'", k.as_str()))
        .collect::<Vec<_>>()
        .join(",");

    let mut candidates = Vec::new();
    // Full-text match.
    if opts.per_kind.is_some() {
        // The best candidates of each kind, so a crowded kind does not take
        // every slot: bm25 is computed once for all matches either way.
        let sql = format!(
            "SELECT entity, ref_id, kind, text, plain_key, loose_key, weight, rank FROM \
             (SELECT d.entity, d.ref_id, d.kind, d.text, d.plain_key, d.loose_key, d.weight, \
                     f.rank AS rank, \
                     row_number() OVER (PARTITION BY d.kind ORDER BY f.rank) AS n \
              FROM search_fts f JOIN search_docs d ON d.id = f.rowid \
              WHERE search_fts MATCH ?1 AND d.kind IN ({kind_list})) \
             WHERE n <= ?2"
        );
        collect(
            conn,
            &sql,
            params![q.fts_expr(), FTS_CANDIDATES as i64],
            &mut candidates,
        )?;
    } else {
        let sql = format!(
            "SELECT d.entity, d.ref_id, d.kind, d.text, d.plain_key, d.loose_key, d.weight, f.rank \
             FROM (SELECT rowid, rank FROM search_fts WHERE search_fts MATCH ?1 ORDER BY rank LIMIT ?2) f \
             JOIN search_docs d ON d.id = f.rowid WHERE d.kind IN ({kind_list})"
        );
        // Without a kind filter the inner LIMIT is exact; with one, fetch more.
        let fts_limit = if opts.kinds.is_empty() {
            FTS_CANDIDATES
        } else {
            FTS_CANDIDATES * 4
        };
        collect(
            conn,
            &sql,
            params![q.fts_expr(), fts_limit as i64],
            &mut candidates,
        )?;
    }

    // Exact and starts-with lookups: short titles that bm25 does not favour
    // (`Up` among thousands of `up*`).
    let n = q.n();
    let head_plain = q.plain[..n - 1].join(" ");
    let head_loose = q.loose[..n - 1].join(" ");
    let with_head = |head: &str, last: &str| {
        if head.is_empty() {
            last.to_owned()
        } else {
            format!("{head} {last}")
        }
    };
    let mut lookups: Vec<(&str, String)> = Vec::new();
    for p in &q.plain_prefixes {
        lookups.push(("plain_key", with_head(&head_plain, p)));
    }
    for p in &q.loose_prefixes {
        lookups.push(("loose_key", with_head(&head_loose, p)));
    }
    for (col, prefix) in lookups {
        let sql = format!(
            "SELECT d.entity, d.ref_id, d.kind, d.text, d.plain_key, d.loose_key, d.weight, 0.0 \
             FROM search_docs d \
             WHERE d.{col} >= ?1 AND d.{col} < ?1 || char(1114111) AND d.kind IN ({kind_list}) \
             LIMIT ?2"
        );
        collect(
            conn,
            &sql,
            params![prefix, KEY_CANDIDATES as i64],
            &mut candidates,
        )?;
    }

    // Score, one hit per entity.
    let mut best: HashMap<(String, i64), (f64, Candidate)> = HashMap::new();
    for c in candidates {
        let s = score(&q, &c);
        let key = (c.entity.clone(), c.ref_id);
        match best.get(&key) {
            Some((old, _)) if *old >= s => {}
            _ => {
                best.insert(key, (s, c));
            }
        }
    }
    let mut ranked: Vec<(f64, Candidate)> = best.into_values().collect();
    ranked.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.text.len().cmp(&b.1.text.len()))
            .then_with(|| a.1.ref_id.cmp(&b.1.ref_id))
    });

    let mut hits = Vec::with_capacity(opts.limit.min(ranked.len()));
    let mut per_kind: HashMap<SearchKind, usize> = HashMap::new();
    for (score, c) in ranked {
        if hits.len() == opts.limit {
            break;
        }
        if let Some(cap) = opts.per_kind
            && per_kind.get(&c.kind).copied().unwrap_or(0) >= cap
        {
            continue;
        }
        let kind = c.kind;
        if let Some(hit) = details(conn, c, score, opts)? {
            hits.push(hit);
            *per_kind.entry(kind).or_default() += 1;
        }
    }
    Ok(hits)
}

fn collect(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    out: &mut Vec<Candidate>,
) -> Result<(), Error> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(params, |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, f64>(6)?,
            r.get::<_, f64>(7)?,
        ))
    })?;
    for row in rows {
        let (entity, ref_id, kind, text, plain_key, loose_key, weight, bm25) = row?;
        let Some(kind) = SearchKind::parse(&kind) else {
            continue;
        };
        out.push(Candidate {
            entity,
            ref_id,
            kind,
            text,
            plain_key,
            loose_key,
            weight,
            bm25,
        });
    }
    Ok(())
}

/// How a document matches: 3 — the whole title, 2 — the title starts with
/// the query, 1 — the tokens are somewhere in it.
fn tier(q: &Query, plain: &[&str], loose: &[&str]) -> u8 {
    let n = q.n();
    let eq = |doc: &[&str], full: &[String]| {
        doc.len() == full.len() && doc.iter().zip(full).all(|(a, b)| a == b)
    };
    if eq(plain, &q.plain) || eq(loose, &q.loose) || loose.concat() == q.loose.concat() {
        return 3;
    }
    let starts = |doc: &[&str], full: &[String], prefixes: &[String]| {
        doc.len() >= n
            && doc[..n - 1].iter().zip(&full[..n - 1]).all(|(a, b)| a == b)
            && prefixes.iter().any(|p| doc[n - 1].starts_with(p.as_str()))
    };
    if starts(plain, &q.plain, &q.plain_prefixes) || starts(loose, &q.loose, &q.loose_prefixes) || {
        let head = q.loose[..n - 1].concat();
        let doc = loose.concat();
        q.loose_prefixes
            .iter()
            .any(|p| doc.starts_with(&format!("{head}{p}")))
    } {
        return 2;
    }
    1
}

fn score(q: &Query, c: &Candidate) -> f64 {
    let plain: Vec<&str> = c.plain_key.split(' ').collect();
    let loose: Vec<&str> = c.loose_key.split(' ').collect();
    let tier = tier(q, &plain, &loose);
    // Share of the title covered by the query: `Up` beats `Up in the Air`.
    let q_len: usize = q.loose.iter().map(String::len).sum();
    let d_len: usize = loose.iter().map(|t| t.len()).sum();
    let coverage = (q_len as f64 / d_len.max(1) as f64).min(1.0);
    // bm25 is negative, lower is better: a tie-breaker in (0, 0.05), below
    // the smallest difference of kind weights.
    let bm25 = if c.bm25 < 0.0 {
        0.05 * (-c.bm25) / (1.0 - c.bm25)
    } else {
        0.0
    };
    f64::from(tier) * 10.0 + coverage * 3.0 + c.weight + c.kind.weight() + bm25
}

/// Display fields of a candidate; `None` if the row is gone or hidden from
/// the profile.
fn details(
    conn: &Connection,
    c: Candidate,
    score: f64,
    opts: &SearchOptions,
) -> Result<Option<SearchHit>, Error> {
    let id = c.ref_id;
    let row: Option<(String, Option<String>, Option<i64>)> = match c.kind {
        SearchKind::Movie | SearchKind::Show | SearchKind::Anime => conn
            .prepare_cached("SELECT title, NULL, year FROM works WHERE id = ?1")?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?,
        SearchKind::Episode => conn
            .prepare_cached(
                "SELECT e.title, w.title, e.season_number, e.episode_number, \
                        CAST(substr(e.air_date, 1, 4) AS INTEGER) \
                 FROM episodes e JOIN works w ON w.id = e.work_id WHERE e.id = ?1",
            )?
            .query_row([id], |r| {
                let title: Option<String> = r.get(0)?;
                let show: String = r.get(1)?;
                let s: Option<i64> = r.get(2)?;
                let e: Option<i64> = r.get(3)?;
                let number = match (s, e) {
                    (Some(s), Some(e)) => Some(format!("S{s:02}E{e:02}")),
                    (None, Some(e)) => Some(format!("E{e:02}")),
                    _ => None,
                };
                let subtitle = match &number {
                    Some(n) => format!("{show} · {n}"),
                    None => show,
                };
                let title = title.or(number).unwrap_or_default();
                Ok((title, Some(subtitle), r.get(4)?))
            })
            .optional()?,
        SearchKind::Video => conn
            .prepare_cached("SELECT path FROM files WHERE id = ?1")?
            .query_row([id], |r| {
                let path: String = r.get(0)?;
                let name = split_path(&path).1;
                let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).to_owned();
                Ok((stem, Some(path), None))
            })
            .optional()?,
        SearchKind::Artist => conn
            .prepare_cached("SELECT name, NULL, NULL FROM music_artists WHERE id = ?1")?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?,
        SearchKind::Album => conn
            .prepare_cached(
                "SELECT al.title, \
                        (SELECT group_concat(name, ', ') FROM \
                            (SELECT a.name FROM music_album_artists aa \
                             JOIN music_artists a ON a.id = aa.artist_id \
                             WHERE aa.album_id = al.id ORDER BY aa.position)), \
                        al.year \
                 FROM music_albums al WHERE al.id = ?1",
            )?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?,
        SearchKind::Track => conn
            .prepare_cached(
                "SELECT t.title, \
                        (SELECT a.name FROM music_track_artists ta \
                         JOIN music_artists a ON a.id = ta.artist_id \
                         WHERE ta.track_id = t.id AND ta.role = 'artist' \
                         ORDER BY ta.position LIMIT 1), \
                        al.title, t.year \
                 FROM music_tracks t LEFT JOIN music_albums al ON al.id = t.album_id \
                 WHERE t.id = ?1",
            )?
            .query_row([id], |r| {
                let artist: Option<String> = r.get(1)?;
                let album: Option<String> = r.get(2)?;
                let subtitle = match (artist, album) {
                    (Some(a), Some(b)) => Some(format!("{a} · {b}")),
                    (a, b) => a.or(b),
                };
                Ok((r.get(0)?, subtitle, r.get(3)?))
            })
            .optional()?,
        SearchKind::Collection => conn
            .prepare_cached(
                "SELECT name, NULL, NULL FROM collections \
                 WHERE id = ?1 AND (?2 IS NULL OR profile_id IS NULL OR profile_id = ?2)",
            )?
            .query_row(params![id, opts.profile_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?,
    };
    Ok(row.map(|(title, subtitle, year)| SearchHit {
        kind: c.kind,
        id,
        title,
        subtitle,
        year,
        matched: c.text,
        score,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_expression() {
        let q = Query::parse("Ирония судь").unwrap();
        assert_eq!(q.plain, ["ирония", "судь"]);
        assert_eq!(q.loose[0], "ironya");
        assert_eq!(
            q.fts_expr(),
            "(plain:\"ирония\" OR loose:\"ironya\") AND \
             (plain:\"судь\"* OR loose:\"sud\"*)"
        );
        assert!(Query::parse("  ?! ").is_none());
        // Roman numerals: the typed form and the arabic one.
        let q = Query::parse("rocky iv").unwrap();
        assert_eq!(q.plain_prefixes, ["iv", "4"]);
    }

    #[test]
    fn partial_words_in_the_other_script() {
        assert_eq!(partial_loose("ирони").as_deref(), Some("iron"));
        assert_eq!(partial_loose("ведьм").as_deref(), Some("vedm"));
        assert_eq!(partial_loose("a"), None);
        assert!(TitleKey::new("ironiya").loose.starts_with("iron"));
    }

    #[test]
    fn doc_keys() {
        let k = DocKeys::new("Spider-Man: No Way Home").unwrap();
        assert_eq!(k.plain, ["spider", "man", "no", "way", "home"]);
        assert!(k.loose_column().ends_with(" spidermannowayhome"));
        assert_eq!(
            DocKeys::new("Война и мир").unwrap().loose,
            ["voyna", "1", "mir"]
        );
        assert!(DocKeys::new("!!!").is_none());
    }
}
