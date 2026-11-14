// SPDX-License-Identifier: MPL-2.0
//! Local recognition: video files → works, episodes and matches, from the
//! `pa-recognition` parser alone (no providers).
//!
//! For every video file of a source that has no live match:
//!
//! - the relative path is parsed (folders give context); the N-best result
//!   goes to `parse_results`;
//! - the best hypothesis decides: an episode (season / episode / absolute
//!   markers) joins the show work with the same title key (created on first
//!   sight; `anime` if the name looks like an anime release), a movie joins
//!   the movie work with the same title key and year; extras are skipped;
//! - the file gets a live match (`state = auto`, `decided_by = rule`,
//!   confidence = hypothesis weight) and, for episodes, `match_episodes`.
//!
//! Title keys are the loose keys of `pa_recognition::title::TitleKey`, so
//! `Ведьмак`, `Vedmak`, `The Witcher` spelled differently in two folders
//! stay apart but case, punctuation and transliteration do not split a
//! show. All titles of a work (`titles`) are keys, so a renamed work keeps
//! catching files named the old way.

use std::collections::HashMap;

use pa_library::rusqlite::{Connection, OptionalExtension, Transaction, params};
use pa_recognition::parse::{self, EpisodeKind, MediaHint, ParsedName};
use pa_recognition::title::TitleKey;
use pa_scanner::MediaKind;

use crate::Error;

/// Files recognized per transaction.
const BATCH: usize = 200;

/// Title key of a work: kind group, loose title and (movies) year.
fn work_key(series: bool, title: &str, year: Option<i64>) -> String {
    let key = TitleKey::new(title);
    let loose = if key.loose.is_empty() {
        key.plain
    } else {
        key.loose
    };
    if series {
        format!("series|{loose}")
    } else {
        format!("movie|{loose}|{}", year.unwrap_or(0))
    }
}

/// Lookup of existing works by title key.
pub(crate) struct WorkIndex {
    keys: HashMap<String, i64>,
}

impl WorkIndex {
    pub(crate) fn load(conn: &Connection) -> Result<Self, Error> {
        let mut keys = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT w.id, w.kind, w.year, w.title FROM works w
             UNION ALL
             SELECT w.id, w.kind, w.year, t.text FROM titles t JOIN works w ON w.id = t.work_id
             ORDER BY 1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, kind, year, title) = row?;
            let series = kind != "movie";
            keys.entry(work_key(series, &title, year)).or_insert(id);
            if !series {
                // A file without a year joins a movie with one.
                keys.entry(work_key(false, &title, None)).or_insert(id);
            }
        }
        Ok(Self { keys })
    }

    /// The work for `title` (and `year` for movies), created if needed.
    pub(crate) fn ensure(
        &mut self,
        conn: &Connection,
        kind: &str,
        title: &str,
        year: Option<i64>,
        now: i64,
    ) -> Result<i64, Error> {
        let series = kind != "movie";
        let key = work_key(series, title, year);
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        if !series
            && year.is_none()
            && let Some(id) = self.keys.get(&work_key(false, title, None))
        {
            return Ok(*id);
        }
        let year = year.filter(|y| (1800..=3000).contains(y));
        conn.execute(
            "INSERT INTO works (kind, title, year, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![kind, title, year, now],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT OR IGNORE INTO titles (work_id, text, kind, source) VALUES (?1, ?2, 'main', 'local')",
            params![id, title],
        )?;
        self.keys.insert(key, id);
        if !series {
            self.keys.entry(work_key(false, title, None)).or_insert(id);
        }
        Ok(id)
    }
}

/// Numbers of one catalog episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EpisodeNo {
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub absolute: Option<i64>,
    pub kind: &'static str,
}

/// The episode of `work_id` with these numbers, created if needed.
pub(crate) fn ensure_episode(
    conn: &Connection,
    work_id: i64,
    no: EpisodeNo,
    title: Option<&str>,
) -> Result<i64, Error> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM episodes
             WHERE work_id = ?1 AND season_number IS ?2 AND episode_number IS ?3
               AND absolute_number IS ?4 AND episode_type = ?5
             ORDER BY id LIMIT 1",
            params![work_id, no.season, no.episode, no.absolute, no.kind],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        if let Some(t) = title {
            conn.execute(
                "UPDATE episodes SET title = ?2 WHERE id = ?1 AND title IS NULL",
                params![id, t],
            )?;
        }
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO episodes (work_id, episode_type, season_number, episode_number,
                               absolute_number, title)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![work_id, no.kind, no.season, no.episode, no.absolute, title],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Live match of `file_id` → `work_id` with its episodes in file order.
/// Does nothing if the file already has a live match.
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_match(
    conn: &Connection,
    file_id: i64,
    work_id: i64,
    episodes: &[i64],
    confidence: f64,
    state: &str,
    decided_by: &str,
    model_version: Option<&str>,
    now: i64,
) -> Result<bool, Error> {
    let inserted = conn.execute(
        "INSERT INTO matches (file_id, work_id, scheme_used, confidence, state, decided_by,
                              model_version, created_at_ms, updated_at_ms)
         SELECT ?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?7
         WHERE NOT EXISTS (SELECT 1 FROM matches WHERE file_id = ?1 AND state <> 'rejected')",
        params![
            file_id,
            work_id,
            confidence.clamp(0.0, 1.0),
            state,
            decided_by,
            model_version,
            now
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }
    let match_id = conn.last_insert_rowid();
    for (pos, ep) in episodes.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO match_episodes (match_id, position, episode_id) VALUES (?1, ?2, ?3)",
            params![match_id, i64::try_from(pos).unwrap_or(i64::MAX), ep],
        )?;
    }
    Ok(true)
}

fn episode_type(kind: Option<EpisodeKind>) -> &'static str {
    match kind {
        None | Some(EpisodeKind::Regular) => "regular",
        Some(EpisodeKind::Special | EpisodeKind::Ova | EpisodeKind::Ona | EpisodeKind::Sp) => {
            "special"
        }
        Some(EpisodeKind::Recap) => "recap",
        Some(EpisodeKind::Ncop | EpisodeKind::Nced) => "credit",
    }
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// A lowercase file-name title (`inception.mkv`) takes the casing of the
/// same title written in an alternative title or in its folder
/// (`Inception (2010)/`).
fn better_cased(title: String, alts: &[String], path: &str) -> String {
    if title.chars().any(char::is_uppercase) {
        return title;
    }
    let key = TitleKey::new(&title).loose;
    let mut folders = path.rsplit('/').skip(1).take(2).map(|f| {
        // `Inception (2010) [1080p]` → `Inception`.
        f.split(['(', '[']).next().unwrap_or(f).trim().to_owned()
    });
    alts.iter()
        .cloned()
        .chain(&mut folders)
        .find(|a| a.chars().any(char::is_uppercase) && TitleKey::new(a).loose == key)
        .unwrap_or(title)
}

/// What the parser says a file is.
enum Plan {
    Skip,
    Movie {
        title: String,
        year: Option<i64>,
    },
    Episodes {
        anime: bool,
        title: String,
        year: Option<i64>,
        numbers: Vec<EpisodeNo>,
        episode_title: Option<String>,
    },
}

fn plan(path: &str, p: &ParsedName) -> Plan {
    if p.media == MediaHint::Extra || p.extra.is_some() {
        return Plan::Skip;
    }
    let title = p
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| file_stem(path).to_owned());
    let title = better_cased(title, &p.alt_titles, path);
    let year = p.year.map(i64::from);
    let is_episode = p.media == MediaHint::Episode
        || !p.episodes.is_empty()
        || !p.absolute.is_empty()
        || p.date.is_some();
    if !is_episode {
        return Plan::Movie { title, year };
    }
    let kind = episode_type(p.episode_kind);
    let season = p.season.map(i64::from).or_else(|| {
        // A season qualifier in the show title (`Mob Psycho 100 III`).
        TitleKey::new(&title).season.map(i64::from)
    });
    let mut numbers: Vec<EpisodeNo> = p
        .episodes
        .iter()
        .filter(|e| e.fraction.is_none())
        .map(|e| EpisodeNo {
            season: season.or(Some(1)),
            episode: Some(i64::from(e.number)),
            absolute: None,
            kind,
        })
        .collect();
    if numbers.is_empty() {
        numbers = p
            .absolute
            .iter()
            .filter(|e| e.fraction.is_none())
            .map(|e| EpisodeNo {
                season,
                episode: None,
                absolute: Some(i64::from(e.number)),
                kind,
            })
            .collect();
    }
    if numbers.is_empty() {
        // A dated or unnumbered episode: its own catalog row.
        numbers.push(EpisodeNo {
            season,
            episode: None,
            absolute: None,
            kind,
        });
    }
    Plan::Episodes {
        anime: p.anime_hint,
        title,
        year,
        numbers,
        episode_title: p.episode_title.clone(),
    }
}

/// Recognizes the video files of `source_id` without a live match. Returns
/// how many got one.
pub(crate) fn run(
    conn: &mut Connection,
    source_id: i64,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<usize, Error> {
    let files: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT f.id, f.path FROM files f
             WHERE f.source_id = ?1 AND f.missing_since_ms IS NULL
               AND NOT EXISTS (SELECT 1 FROM matches m WHERE m.file_id = f.id AND m.state <> 'rejected')
             ORDER BY f.path",
        )?;
        stmt.query_map([source_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter(|r| {
                r.as_ref().map_or(true, |(_, p): &(i64, String)| {
                    pa_scanner::media_kind(p) == Some(MediaKind::Video)
                })
            })
            .collect::<Result<_, _>>()?
    };
    let total = files.len();
    let mut index = WorkIndex::load(conn)?;
    let mut matched = 0;
    for (n, batch) in files.chunks(BATCH).enumerate() {
        let tx = conn.transaction()?;
        for (file_id, path) in batch {
            if recognize_file(&tx, &mut index, *file_id, path)? {
                matched += 1;
            }
        }
        tx.commit()?;
        on_progress(((n + 1) * BATCH).min(total), total);
    }
    Ok(matched)
}

fn recognize_file(
    tx: &Transaction<'_>,
    index: &mut WorkIndex,
    file_id: i64,
    path: &str,
) -> Result<bool, Error> {
    let now = crate::now_ms();
    let result = parse::parse(path);
    let hypotheses = serde_json::to_string(&result.hypotheses)
        .map_err(|e| Error::Invalid(format!("parse result: {e}")))?;
    tx.execute(
        "INSERT INTO parse_results (file_id, parser_version, hypotheses_json, parsed_at_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (file_id) DO UPDATE SET parser_version = excluded.parser_version,
             hypotheses_json = excluded.hypotheses_json, parsed_at_ms = excluded.parsed_at_ms",
        params![file_id, result.parser_version, hypotheses, now],
    )?;
    let best = result.best();
    let confidence = f64::from(best.weight);
    let version = Some(result.parser_version.as_str());
    match plan(path, &best.parsed) {
        Plan::Skip => Ok(false),
        Plan::Movie { title, year } => {
            let work = index.ensure(tx, "movie", &title, year, now)?;
            insert_match(
                tx,
                file_id,
                work,
                &[],
                confidence,
                "auto",
                "rule",
                version,
                now,
            )
        }
        Plan::Episodes {
            anime,
            title,
            year,
            numbers,
            episode_title,
        } => {
            let kind = if anime { "anime" } else { "show" };
            let work = index.ensure(tx, kind, &title, year, now)?;
            if let Some(y) = year.filter(|y| (1800..=3000).contains(y)) {
                tx.execute(
                    "UPDATE works SET year = ?2 WHERE id = ?1 AND year IS NULL",
                    params![work, y],
                )?;
            }
            let single = numbers.len() == 1;
            let mut eps = Vec::with_capacity(numbers.len());
            for no in numbers {
                let t = if single {
                    episode_title.as_deref()
                } else {
                    None
                };
                eps.push(ensure_episode(tx, work, no, t)?);
            }
            insert_match(
                tx, file_id, work, &eps, confidence, "auto", "rule", version, now,
            )
        }
    }
}

/// Deletes works without any match and episodes without any file, after
/// files were removed. Works the user touched (a `user_*` match, which
/// cascades away with its file) go too: their files are gone.
pub(crate) fn remove_orphans(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "DELETE FROM works WHERE NOT EXISTS (SELECT 1 FROM matches m WHERE m.work_id = works.id)",
        [],
    )?;
    conn.execute(
        "DELETE FROM episodes WHERE NOT EXISTS
             (SELECT 1 FROM match_episodes me WHERE me.episode_id = episodes.id)",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_of(path: &str) -> Plan {
        plan(path, &parse::parse(path).best().parsed)
    }

    #[test]
    fn movie_and_episode_plans() {
        match plan_of("Movies/Dune (2021)/Dune.2021.2160p.mkv") {
            Plan::Movie { title, year } => {
                assert_eq!(title, "Dune");
                assert_eq!(year, Some(2021));
            }
            _ => panic!("movie expected"),
        }
        match plan_of("Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.mkv") {
            Plan::Episodes { title, numbers, .. } => {
                assert_eq!(title, "Dark");
                assert_eq!(numbers[0].season, Some(1));
                assert_eq!(numbers[0].episode, Some(2));
                assert_eq!(numbers[0].kind, "regular");
            }
            _ => panic!("episode expected"),
        }
    }

    #[test]
    fn folder_casing_wins_over_a_lowercase_file_name() {
        match plan_of("Movies/Inception (2010)/inception.mkv") {
            Plan::Movie { title, year } => {
                assert_eq!(title, "Inception");
                assert_eq!(year, Some(2010));
            }
            _ => panic!("movie expected"),
        }
    }

    #[test]
    fn work_keys_fold_case_and_punctuation() {
        assert_eq!(
            work_key(true, "The Office", None),
            work_key(true, "the office!", None)
        );
        assert_ne!(
            work_key(false, "Dune", Some(1984)),
            work_key(false, "Dune", Some(2021))
        );
    }
}
