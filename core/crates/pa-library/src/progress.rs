// SPDX-License-Identifier: MPL-2.0
//! Playback progress, history, "Continue watching" and Up Next (LIB-011,
//! docs/research/01 §2.7).
//!
//! The `progress` table holds one [`PlaybackItem`] per profile and
//! [`MediaKey`]. Rules (the same as release 0.1, `pa_library` Dart):
//!
//! - an item is **in progress** while `min <= position / duration <= max`
//!   ([`Thresholds`], default 2 % and 95 %); only such items are in the
//!   "Continue watching" row;
//! - a position report beyond `max` marks the item **watched**. "Watched" is
//!   monotonic: later position reports never clear it, only an explicit
//!   [`mark_unwatched`] does;
//! - the row is sorted by `updated_at_ms`, latest first, and merges movies,
//!   episodes, unrecognised videos and Twitch VODs and recordings. Live
//!   streams have no position and never get here; tracks and clips are left
//!   out by default ([`CONTINUE_KINDS`]);
//! - a show whose last watched regular episode is finished gets an **Up
//!   Next** entry: the next unwatched episode that has a file
//!   ([`next_episode`]);
//! - "Remove from row" ([`hide_from_continue`]) hides an entry until the item
//!   is played again. It is local to the device.
//!
//! Local changes go through a [`ProgressSink`] in the same transaction as the
//! table write: `pa-sync` implements it for its `Replica`, which turns every
//! [`ProgressChange`] into a sync op (`progress.upsert`, `watched`,
//! `unwatch`) in the outbox. Changes received from other devices are written
//! with [`apply_remote`].
//!
//! ```
//! use pa_library::progress::{self, LocalOnly, PositionReport, Thresholds};
//! use pa_model::MediaKind;
//!
//! let mut conn = pa_library::open_in_memory().unwrap();
//! let mut sink = LocalOnly::new("tv");
//! let report = PositionReport {
//!     profile_id: 1,
//!     key: "tmdb:movie:603".parse().unwrap(),
//!     kind: MediaKind::Movie,
//!     position_ms: 2_000_000,
//!     duration_ms: Some(8_160_000),
//! };
//! let th = Thresholds::default();
//! progress::report_position(&mut conn, &mut sink, &th, &report, 1_763_000_000_000).unwrap();
//! let row = progress::continue_watching(&conn, &th, 1, &Default::default()).unwrap();
//! assert_eq!(row[0].item.position_ms, 2_000_000);
//! ```

use std::collections::HashSet;

use pa_model::{IN_PROGRESS_MAX, IN_PROGRESS_MIN, MediaKey, MediaKind, PlaybackItem};
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::Error;

/// Error type a [`ProgressSink`] may return.
pub type SinkError = Box<dyn std::error::Error + Send + Sync>;

/// Kinds in the "Continue watching" row by default: everything with a
/// resumable position except music tracks and Twitch clips.
pub const CONTINUE_KINDS: [MediaKind; 5] = [
    MediaKind::Movie,
    MediaKind::Episode,
    MediaKind::Video,
    MediaKind::TwitchVod,
    MediaKind::TwitchRecording,
];

/// Default size of the "Continue watching" row.
pub const DEFAULT_CONTINUE_LIMIT: usize = 50;

/// How many watched episode rows the Up Next pass looks at.
const UP_NEXT_SCAN: i64 = 1_000;

/// "In progress" window and the "watched" bound, as fractions of the
/// duration. Configurable within the ranges of 01 §2.7 (2–3 % and 90–95 %)
/// or wider; the defaults are those of pa-model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Lower bound of "in progress" (inclusive).
    pub in_progress_min: f64,
    /// Upper bound of "in progress" (inclusive); a position beyond it marks
    /// the item watched.
    pub watched_after: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            in_progress_min: IN_PROGRESS_MIN,
            watched_after: IN_PROGRESS_MAX,
        }
    }
}

impl Thresholds {
    /// Checked thresholds: `0 <= min < max <= 1`.
    pub fn new(in_progress_min: f64, watched_after: f64) -> Result<Self, Error> {
        let ok = (0.0..1.0).contains(&in_progress_min)
            && in_progress_min < watched_after
            && watched_after <= 1.0;
        if ok {
            Ok(Self {
                in_progress_min,
                watched_after,
            })
        } else {
            Err(Error::Invalid("progress thresholds"))
        }
    }

    /// True if the position is inside the "in progress" window. False when
    /// the duration is unknown.
    pub fn is_in_progress(&self, position_ms: u64, duration_ms: Option<u64>) -> bool {
        fraction(position_ms, duration_ms)
            .is_some_and(|p| p >= self.in_progress_min && p <= self.watched_after)
    }

    /// True if the position is past the "watched" bound.
    pub fn is_complete(&self, position_ms: u64, duration_ms: Option<u64>) -> bool {
        fraction(position_ms, duration_ms).is_some_and(|p| p > self.watched_after)
    }
}

/// Position as a fraction of a known, non-zero duration (not clamped, so it
/// matches the SQL filter of [`continue_watching`]).
fn fraction(position_ms: u64, duration_ms: Option<u64>) -> Option<f64> {
    let d = duration_ms.filter(|d| *d > 0)?;
    Some(position_ms as f64 / d as f64)
}

/// A local change of playback state, as handed to a [`ProgressSink`].
/// `profile` is `profiles.sync_id`. Maps one to one onto the pa-sync ops
/// `progress.upsert`, `watched` and `unwatch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressChange {
    /// New position. Never changes "watched".
    Position {
        profile: String,
        key: MediaKey,
        kind: MediaKind,
        position_ms: u64,
        duration_ms: Option<u64>,
    },
    /// The item became watched.
    Watched { profile: String, key: MediaKey },
    /// "Watched" was cleared explicitly.
    Unwatched { profile: String, key: MediaKey },
}

/// Receives local progress changes inside the transaction that writes them,
/// e.g. to queue sync ops in the outbox of the same database. An error rolls
/// the local change back.
pub trait ProgressSink {
    /// Id of this device, stored in `progress.device_id`.
    fn device_id(&self) -> &str;

    /// Called once per change, in order, with the open transaction.
    fn record_change(
        &mut self,
        conn: &Connection,
        change: ProgressChange,
        now_ms: i64,
    ) -> Result<(), SinkError>;
}

/// A sink that syncs nothing (a single device, tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOnly {
    device_id: String,
}

impl LocalOnly {
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
        }
    }
}

impl ProgressSink for LocalOnly {
    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn record_change(&mut self, _: &Connection, _: ProgressChange, _: i64) -> Result<(), SinkError> {
        Ok(())
    }
}

/// A position report from the player (every 5–10 s, on pause and exit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionReport {
    /// `profiles.id`.
    pub profile_id: i64,
    pub key: MediaKey,
    pub kind: MediaKind,
    pub position_ms: u64,
    /// `None` keeps the stored duration.
    pub duration_ms: Option<u64>,
}

/// Why an entry is in the "Continue watching" row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinueReason {
    /// Started and not finished.
    InProgress,
    /// The next episode after the last finished one.
    UpNext,
}

/// An episode of the catalog, as used by Up Next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeRef {
    pub episode_id: i64,
    pub work_id: i64,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    /// Key to play and to store progress under.
    pub key: MediaKey,
    pub title: Option<String>,
    pub runtime_ms: Option<u64>,
}

/// One entry of the "Continue watching" row.
#[derive(Debug, Clone, PartialEq)]
pub struct ContinueEntry {
    /// What to show and resume. For Up Next: the next episode, its own
    /// position if it has one (else 0), and `updated_at_ms` and `device_id`
    /// of the finished episode that led to it.
    pub item: PlaybackItem,
    pub reason: ContinueReason,
    /// The catalog episode, for episodes the catalog knows.
    pub episode: Option<EpisodeRef>,
    /// Key of the `progress` row behind the entry; pass it to
    /// [`hide_from_continue`] for "Remove from row". For Up Next it is the
    /// finished episode.
    pub anchor_key: MediaKey,
}

/// Options of [`continue_watching`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueQuery {
    /// Kinds to include. Sections use a subset (Movies, TV, Twitch).
    pub kinds: Vec<MediaKind>,
    /// Add Up Next entries (needs [`MediaKind::Episode`] in `kinds`).
    pub up_next: bool,
    pub limit: usize,
}

impl Default for ContinueQuery {
    fn default() -> Self {
        Self {
            kinds: CONTINUE_KINDS.to_vec(),
            up_next: true,
            limit: DEFAULT_CONTINUE_LIMIT,
        }
    }
}

/// Text form of a kind, as stored in `progress.source_type`.
pub fn kind_str(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "movie",
        MediaKind::Episode => "episode",
        MediaKind::Track => "track",
        MediaKind::TwitchVod => "twitch_vod",
        MediaKind::TwitchRecording => "twitch_recording",
        MediaKind::TwitchClip => "twitch_clip",
        MediaKind::Video => "video",
    }
}

fn parse_kind(s: &str) -> Result<MediaKind, Error> {
    Ok(match s {
        "movie" => MediaKind::Movie,
        "episode" => MediaKind::Episode,
        "track" => MediaKind::Track,
        "twitch_vod" => MediaKind::TwitchVod,
        "twitch_recording" => MediaKind::TwitchRecording,
        "twitch_clip" => MediaKind::TwitchClip,
        "video" => MediaKind::Video,
        _ => return Err(Error::Invalid("source type")),
    })
}

/// The kind a key implies; used when a row is created without one (a
/// "watched" mark for an item never played here).
pub fn kind_of_key(key: &MediaKey) -> MediaKind {
    match key {
        MediaKey::File { .. } => MediaKind::Video,
        MediaKey::TmdbMovie { .. } => MediaKind::Movie,
        MediaKey::TmdbEpisode { .. } => MediaKind::Episode,
        MediaKey::MbRecording { .. } => MediaKind::Track,
        MediaKey::TwitchVod { .. } => MediaKind::TwitchVod,
        MediaKey::TwitchRecording { .. } => MediaKind::TwitchRecording,
        MediaKey::TwitchClip { .. } => MediaKind::TwitchClip,
    }
}

fn to_i64(v: u64, what: &'static str) -> Result<i64, Error> {
    i64::try_from(v).map_err(|_| Error::Invalid(what))
}

fn opt_u64(v: Option<i64>) -> Option<u64> {
    v.and_then(|v| u64::try_from(v).ok())
}

fn check_key(key: &MediaKey) -> Result<String, Error> {
    if key.is_valid() {
        Ok(key.to_string())
    } else {
        Err(Error::Invalid("media key"))
    }
}

fn profile_sync_id(conn: &Connection, profile_id: i64) -> Result<String, Error> {
    conn.query_row(
        "SELECT sync_id FROM profiles WHERE id = ?1",
        [profile_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(Error::Invalid("profile"))
}

const ITEM_COLUMNS: &str =
    "media_key, source_type, position_ms, duration_ms, updated_at_ms, device_id, watched";

/// A row selected with [`ITEM_COLUMNS`] first.
fn item_from_row(r: &Row<'_>) -> rusqlite::Result<Result<PlaybackItem, Error>> {
    let key: String = r.get(0)?;
    let kind: String = r.get(1)?;
    let position: i64 = r.get(2)?;
    let duration: Option<i64> = r.get(3)?;
    let updated_at_ms: i64 = r.get(4)?;
    let device_id: String = r.get(5)?;
    let watched: bool = r.get(6)?;
    Ok((|| {
        Ok(PlaybackItem {
            media_key: key.parse().map_err(|_| Error::Invalid("stored media key"))?,
            source_type: parse_kind(&kind)?,
            position_ms: u64::try_from(position).unwrap_or(0),
            duration_ms: opt_u64(duration),
            updated_at_ms,
            device_id,
            watched,
        })
    })())
}

/// Playback state of `key` on a profile.
pub fn get(conn: &Connection, profile_id: i64, key: &MediaKey) -> Result<Option<PlaybackItem>, Error> {
    let key = check_key(key)?;
    conn.query_row(
        &format!("SELECT {ITEM_COLUMNS} FROM progress WHERE profile_id = ?1 AND media_key = ?2"),
        params![profile_id, key],
        item_from_row,
    )
    .optional()?
    .transpose()
}

/// Records a position report: updates the row (creating it if needed) and,
/// if the position passed [`Thresholds::watched_after`], marks the item
/// watched. Sends [`ProgressChange::Position`] and, on the first crossing,
/// [`ProgressChange::Watched`] to `sink`. Returns the new state.
pub fn report_position(
    conn: &mut Connection,
    sink: &mut dyn ProgressSink,
    thresholds: &Thresholds,
    report: &PositionReport,
    now_ms: i64,
) -> Result<PlaybackItem, Error> {
    let key = check_key(&report.key)?;
    let position = to_i64(report.position_ms, "position")?;
    let duration = report
        .duration_ms
        .map(|d| to_i64(d, "duration"))
        .transpose()?;
    let tx = conn.transaction()?;
    let profile = profile_sync_id(&tx, report.profile_id)?;
    let before = get(&tx, report.profile_id, &report.key)?;
    let duration_ms = report
        .duration_ms
        .or(before.as_ref().and_then(|b| b.duration_ms));
    let was_watched = before.as_ref().is_some_and(|b| b.watched);
    let watched = was_watched || thresholds.is_complete(report.position_ms, duration_ms);
    tx.execute(
        "INSERT INTO progress (profile_id, media_key, source_type, position_ms, duration_ms,
                               watched, updated_at_ms, device_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (profile_id, media_key) DO UPDATE SET
             source_type = excluded.source_type,
             position_ms = excluded.position_ms,
             duration_ms = coalesce(excluded.duration_ms, progress.duration_ms),
             watched = excluded.watched,
             updated_at_ms = excluded.updated_at_ms,
             device_id = excluded.device_id",
        params![
            report.profile_id,
            key,
            kind_str(report.kind),
            position,
            duration,
            watched,
            now_ms,
            sink.device_id(),
        ],
    )?;
    let change = ProgressChange::Position {
        profile: profile.clone(),
        key: report.key.clone(),
        kind: report.kind,
        position_ms: report.position_ms,
        duration_ms,
    };
    sink.record_change(&tx, change, now_ms).map_err(Error::Sync)?;
    if watched && !was_watched {
        let change = ProgressChange::Watched {
            profile,
            key: report.key.clone(),
        };
        sink.record_change(&tx, change, now_ms).map_err(Error::Sync)?;
    }
    let item = get(&tx, report.profile_id, &report.key)?.ok_or(Error::Invalid("progress row"))?;
    tx.commit()?;
    Ok(item)
}

/// "Mark watched". Creates the row if the item was never played here.
/// Returns false (and sends nothing) if it was already watched.
pub fn mark_watched(
    conn: &mut Connection,
    sink: &mut dyn ProgressSink,
    profile_id: i64,
    key: &MediaKey,
    now_ms: i64,
) -> Result<bool, Error> {
    let key_text = check_key(key)?;
    let tx = conn.transaction()?;
    let profile = profile_sync_id(&tx, profile_id)?;
    if get(&tx, profile_id, key)?.is_some_and(|b| b.watched) {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO progress (profile_id, media_key, source_type, position_ms, watched,
                               updated_at_ms, device_id)
         VALUES (?1, ?2, ?3, 0, 1, ?4, ?5)
         ON CONFLICT (profile_id, media_key) DO UPDATE SET
             watched = 1, updated_at_ms = excluded.updated_at_ms, device_id = excluded.device_id",
        params![
            profile_id,
            key_text,
            kind_str(kind_of_key(key)),
            now_ms,
            sink.device_id()
        ],
    )?;
    let change = ProgressChange::Watched {
        profile,
        key: key.clone(),
    };
    sink.record_change(&tx, change, now_ms).map_err(Error::Sync)?;
    tx.commit()?;
    Ok(true)
}

/// "Mark unwatched": clears "watched" and resets the position to 0. Sends
/// [`ProgressChange::Unwatched`] and a zero [`ProgressChange::Position`].
/// Returns false if there was nothing to clear.
pub fn mark_unwatched(
    conn: &mut Connection,
    sink: &mut dyn ProgressSink,
    profile_id: i64,
    key: &MediaKey,
    now_ms: i64,
) -> Result<bool, Error> {
    let key_text = check_key(key)?;
    let tx = conn.transaction()?;
    let profile = profile_sync_id(&tx, profile_id)?;
    let Some(before) = get(&tx, profile_id, key)? else {
        return Ok(false);
    };
    if !before.watched && before.position_ms == 0 {
        return Ok(false);
    }
    tx.execute(
        "UPDATE progress SET watched = 0, position_ms = 0, updated_at_ms = ?3, device_id = ?4
         WHERE profile_id = ?1 AND media_key = ?2",
        params![profile_id, key_text, now_ms, sink.device_id()],
    )?;
    let unwatched = ProgressChange::Unwatched {
        profile: profile.clone(),
        key: key.clone(),
    };
    sink.record_change(&tx, unwatched, now_ms)
        .map_err(Error::Sync)?;
    let position = ProgressChange::Position {
        profile,
        key: key.clone(),
        kind: before.source_type,
        position_ms: 0,
        duration_ms: before.duration_ms,
    };
    sink.record_change(&tx, position, now_ms).map_err(Error::Sync)?;
    tx.commit()?;
    Ok(true)
}

/// "Remove from row": hides the entry whose [`ContinueEntry::anchor_key`]
/// is `key` until the item is played again. Local, not synced. Returns
/// false if there is no such row.
pub fn hide_from_continue(
    conn: &Connection,
    profile_id: i64,
    key: &MediaKey,
    now_ms: i64,
) -> Result<bool, Error> {
    let key = check_key(key)?;
    let n = conn.execute(
        "UPDATE progress SET hidden_at_ms = max(?3, updated_at_ms)
         WHERE profile_id = ?1 AND media_key = ?2",
        params![profile_id, key, now_ms],
    )?;
    Ok(n == 1)
}

/// Watch history of a profile: every item with a position or a "watched"
/// mark, latest change first. `before_ms` pages: pass the `updated_at_ms`
/// of the last item of the previous page.
pub fn history(
    conn: &Connection,
    profile_id: i64,
    before_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<PlaybackItem>, Error> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {ITEM_COLUMNS} FROM progress
         WHERE profile_id = ?1 AND (watched = 1 OR position_ms > 0) AND updated_at_ms < ?2
         ORDER BY updated_at_ms DESC, media_key LIMIT ?3"
    ))?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = stmt.query_map(
        params![profile_id, before_ms.unwrap_or(i64::MAX), limit],
        item_from_row,
    )?;
    rows.map(|r| r?).collect()
}

/// The "Continue watching" row of a profile: in-progress items and Up Next
/// episodes, latest activity first.
pub fn continue_watching(
    conn: &Connection,
    thresholds: &Thresholds,
    profile_id: i64,
    query: &ContinueQuery,
) -> Result<Vec<ContinueEntry>, Error> {
    if query.kinds.is_empty() || query.limit == 0 {
        return Ok(Vec::new());
    }
    let kinds: Vec<&str> = query.kinds.iter().map(|k| kind_str(*k)).collect();
    let marks = vec!["?"; kinds.len()].join(", ");
    let sql = format!(
        "SELECT {ITEM_COLUMNS} FROM progress
         WHERE profile_id = ? AND watched = 0 AND duration_ms > 0
           AND CAST(position_ms AS REAL) / duration_ms >= ?
           AND CAST(position_ms AS REAL) / duration_ms <= ?
           AND (hidden_at_ms IS NULL OR hidden_at_ms < updated_at_ms)
           AND source_type IN ({marks})
         ORDER BY updated_at_ms DESC, media_key LIMIT {UP_NEXT_SCAN}"
    );
    let mut args: Vec<rusqlite::types::Value> = vec![
        profile_id.into(),
        thresholds.in_progress_min.into(),
        thresholds.watched_after.into(),
    ];
    args.extend(kinds.iter().map(|k| (*k).to_owned().into()));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args), item_from_row)?;
    let mut entries = Vec::new();
    let mut busy_works = HashSet::new();
    for row in rows {
        let item = row??;
        let episode = if item.source_type == MediaKind::Episode {
            episode_for_key(conn, &item.media_key)?
                .map(|id| episode_ref(conn, id))
                .transpose()?
                .flatten()
        } else {
            None
        };
        if let Some(ep) = &episode {
            busy_works.insert(ep.work_id);
        }
        entries.push(ContinueEntry {
            anchor_key: item.media_key.clone(),
            item,
            reason: ContinueReason::InProgress,
            episode,
        });
    }
    if query.up_next && query.kinds.contains(&MediaKind::Episode) {
        entries.extend(up_next(conn, profile_id, &busy_works)?);
    }
    entries.sort_by(|a, b| {
        b.item
            .updated_at_ms
            .cmp(&a.item.updated_at_ms)
            .then_with(|| a.anchor_key.cmp(&b.anchor_key))
    });
    entries.truncate(query.limit);
    Ok(entries)
}

/// Up Next entries: per show, the next unwatched episode after the latest
/// finished regular episode. Shows in `busy_works` (an episode is in
/// progress) are skipped.
fn up_next(
    conn: &Connection,
    profile_id: i64,
    busy_works: &HashSet<i64>,
) -> Result<Vec<ContinueEntry>, Error> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {ITEM_COLUMNS}, hidden_at_ms FROM progress
         WHERE profile_id = ?1 AND source_type = 'episode' AND watched = 1
         ORDER BY updated_at_ms DESC, media_key LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![profile_id, UP_NEXT_SCAN], |r| {
        Ok((item_from_row(r)?, r.get::<_, Option<i64>>(7)?))
    })?;
    let mut seen_works = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let (item, hidden_at) = row?;
        let item = item?;
        let Some(episode_id) = episode_for_key(conn, &item.media_key)? else {
            continue;
        };
        let Some(info) = episode_info(conn, episode_id)? else {
            continue;
        };
        // Specials do not move Up Next; the latest regular episode does.
        if !info.is_regular() || !seen_works.insert(info.work_id) {
            continue;
        }
        if busy_works.contains(&info.work_id) || hidden_at.is_some_and(|h| h >= item.updated_at_ms)
        {
            continue;
        }
        let Some(next) = next_unwatched(conn, profile_id, episode_id)? else {
            continue;
        };
        let own = get(conn, profile_id, &next.key)?;
        out.push(ContinueEntry {
            item: PlaybackItem {
                media_key: next.key.clone(),
                source_type: MediaKind::Episode,
                position_ms: own.as_ref().map_or(0, |o| o.position_ms),
                duration_ms: own.and_then(|o| o.duration_ms).or(next.runtime_ms),
                updated_at_ms: item.updated_at_ms,
                device_id: item.device_id,
                watched: false,
            },
            reason: ContinueReason::UpNext,
            episode: Some(next),
            anchor_key: item.media_key,
        });
    }
    Ok(out)
}

/// The next episode after `episode_id` that is not watched on the profile.
fn next_unwatched(
    conn: &Connection,
    profile_id: i64,
    episode_id: i64,
) -> Result<Option<EpisodeRef>, Error> {
    let mut current = episode_id;
    let mut guard = HashSet::new();
    while let Some(next) = next_episode(conn, current)? {
        if !guard.insert(next.episode_id) {
            break;
        }
        if !is_episode_watched(conn, profile_id, next.episode_id)? {
            return Ok(Some(next));
        }
        current = next.episode_id;
    }
    Ok(None)
}

/// Catalog fields of an episode that order it.
struct EpisodeInfo {
    work_id: i64,
    season: Option<i64>,
    number: Option<i64>,
    episode_type: String,
}

impl EpisodeInfo {
    /// A regular episode: type `regular` outside season 0.
    fn is_regular(&self) -> bool {
        self.episode_type == "regular" && self.season != Some(0)
    }
}

fn episode_info(conn: &Connection, episode_id: i64) -> Result<Option<EpisodeInfo>, Error> {
    Ok(conn
        .query_row(
            "SELECT work_id, season_number, ifnull(episode_number, absolute_number), episode_type
             FROM episodes WHERE id = ?1",
            [episode_id],
            |r| {
                Ok(EpisodeInfo {
                    work_id: r.get(0)?,
                    season: r.get(1)?,
                    number: r.get(2)?,
                    episode_type: r.get(3)?,
                })
            },
        )
        .optional()?)
}

/// SQL condition: the episode `e` has a live match to a file that is not
/// missing.
const PLAYABLE: &str = "EXISTS (SELECT 1 FROM match_episodes me
        JOIN matches m ON m.id = me.match_id
        JOIN files f ON f.id = m.file_id
        WHERE me.episode_id = e.id AND m.state <> 'rejected' AND f.missing_since_ms IS NULL)";

/// The episode to play after `episode_id` ("Next Up" in the player), in
/// TMDB order:
///
/// - after a regular episode: the next regular episode, across season
///   boundaries (S1E10 → S2E1); specials (season 0, or a non-regular type)
///   are skipped;
/// - after a special: the next special of season 0 (or the next episode of
///   type `special`); credits, trailers and other extras are never next.
///
/// Episodes without a playable file are skipped. An episode without a
/// season number counts as season 1 and is ordered by its absolute number
/// if it has no episode number.
pub fn next_episode(conn: &Connection, episode_id: i64) -> Result<Option<EpisodeRef>, Error> {
    let Some(info) = episode_info(conn, episode_id)? else {
        return Ok(None);
    };
    let Some(number) = info.number else {
        return Ok(None);
    };
    let sql = if info.is_regular() {
        format!(
            "SELECT e.id FROM episodes e
             WHERE e.work_id = ?1 AND e.episode_type = 'regular' AND ifnull(e.season_number, 1) >= 1
               AND ifnull(e.episode_number, e.absolute_number) IS NOT NULL
               AND (ifnull(e.season_number, 1), ifnull(e.episode_number, e.absolute_number), e.id)
                   > (?2, ?3, ?4)
               AND {PLAYABLE}
             ORDER BY ifnull(e.season_number, 1), ifnull(e.episode_number, e.absolute_number), e.id
             LIMIT 1"
        )
    } else {
        format!(
            "SELECT e.id FROM episodes e
             WHERE e.work_id = ?1
               AND ((e.season_number = 0 AND e.episode_type IN ('regular', 'special'))
                    OR e.episode_type = 'special')
               AND ifnull(e.episode_number, e.absolute_number) IS NOT NULL
               AND (ifnull(e.season_number, 0), ifnull(e.episode_number, e.absolute_number), e.id)
                   > (?2, ?3, ?4)
               AND {PLAYABLE}
             ORDER BY ifnull(e.season_number, 0), ifnull(e.episode_number, e.absolute_number), e.id
             LIMIT 1"
        )
    };
    let season = info.season.unwrap_or(if info.is_regular() { 1 } else { 0 });
    let next: Option<i64> = conn
        .query_row(
            &sql,
            params![info.work_id, season, number, episode_id],
            |r| r.get(0),
        )
        .optional()?;
    match next {
        Some(id) => episode_ref(conn, id),
        None => Ok(None),
    }
}

/// The catalog episode a progress key points to: a TMDB episode key through
/// `external_ids`, a `file:` key through the live match of that file (the
/// last episode of a multi-episode file).
pub fn episode_for_key(conn: &Connection, key: &MediaKey) -> Result<Option<i64>, Error> {
    Ok(match key {
        MediaKey::TmdbEpisode {
            show_id,
            season,
            episode,
        } => conn
            .query_row(
                "SELECT e.id FROM external_ids x JOIN episodes e ON e.work_id = x.work_id
                 WHERE x.provider = 'tmdb' AND x.ext_kind = 'tv' AND x.ext_id = ?1
                   AND e.season_number = ?2 AND e.episode_number = ?3
                 ORDER BY e.id LIMIT 1",
                params![show_id.to_string(), season, episode],
                |r| r.get(0),
            )
            .optional()?,
        MediaKey::File { fingerprint } => conn
            .query_row(
                "SELECT me.episode_id FROM files f
                 JOIN matches m ON m.file_id = f.id AND m.state <> 'rejected'
                 JOIN match_episodes me ON me.match_id = m.id
                 WHERE f.fingerprint = ?1
                 ORDER BY me.position DESC LIMIT 1",
                params![fingerprint],
                |r| r.get(0),
            )
            .optional()?,
        _ => None,
    })
}

/// Keys progress of an episode may be stored under: the TMDB key first (if
/// the show has a TMDB id), then `file:` keys of its matched files.
pub fn episode_keys(conn: &Connection, episode_id: i64) -> Result<Vec<MediaKey>, Error> {
    let mut keys = Vec::new();
    let tmdb: Option<(String, Option<u32>, Option<u32>)> = conn
        .query_row(
            "SELECT x.ext_id, e.season_number, e.episode_number FROM episodes e
             JOIN external_ids x ON x.work_id = e.work_id AND x.provider = 'tmdb' AND x.ext_kind = 'tv'
             WHERE e.id = ?1",
            [episode_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((show, Some(season), Some(episode))) = tmdb
        && let Ok(show_id) = show.parse::<u64>()
    {
        keys.push(MediaKey::TmdbEpisode {
            show_id,
            season,
            episode,
        });
    }
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT f.fingerprint FROM match_episodes me
         JOIN matches m ON m.id = me.match_id AND m.state <> 'rejected'
         JOIN files f ON f.id = m.file_id
         WHERE me.episode_id = ?1 AND f.fingerprint IS NOT NULL
         ORDER BY f.missing_since_ms IS NOT NULL, f.id",
    )?;
    let fingerprints = stmt.query_map([episode_id], |r| r.get::<_, String>(0))?;
    for fp in fingerprints {
        keys.push(MediaKey::File { fingerprint: fp? });
    }
    keys.retain(MediaKey::is_valid);
    Ok(keys)
}

fn episode_ref(conn: &Connection, episode_id: i64) -> Result<Option<EpisodeRef>, Error> {
    let Some(key) = episode_keys(conn, episode_id)?.into_iter().next() else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT work_id, season_number, episode_number, title, runtime_ms
             FROM episodes WHERE id = ?1",
            [episode_id],
            |r| {
                Ok(EpisodeRef {
                    episode_id,
                    work_id: r.get(0)?,
                    season_number: r.get(1)?,
                    episode_number: r.get(2)?,
                    key,
                    title: r.get(3)?,
                    runtime_ms: opt_u64(r.get(4)?),
                })
            },
        )
        .optional()?)
}

/// True if any key of the episode is watched on the profile.
pub fn is_episode_watched(conn: &Connection, profile_id: i64, episode_id: i64) -> Result<bool, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT watched FROM progress WHERE profile_id = ?1 AND media_key = ?2",
    )?;
    for key in episode_keys(conn, episode_id)? {
        let watched: Option<bool> = stmt
            .query_row(params![profile_id, key.to_string()], |r| r.get(0))
            .optional()?;
        if watched == Some(true) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Writes a change received from another device (after the LWW merge of
/// pa-sync). `at_ms` and `device_id` come from the op's HLC. Returns false
/// if the profile is unknown here or the row already has newer data.
///
/// - a position never overwrites a newer row and never touches "watched";
/// - "watched" is monotonic: it is set whatever the row time is;
/// - "unwatched" clears the mark only if it is not older than the row.
pub fn apply_remote(
    conn: &Connection,
    change: &ProgressChange,
    at_ms: i64,
    device_id: &str,
) -> Result<bool, Error> {
    let profile = match change {
        ProgressChange::Position { profile, .. }
        | ProgressChange::Watched { profile, .. }
        | ProgressChange::Unwatched { profile, .. } => profile,
    };
    let Some(profile_id): Option<i64> = conn
        .query_row(
            "SELECT id FROM profiles WHERE sync_id = ?1",
            params![profile],
            |r| r.get(0),
        )
        .optional()?
    else {
        return Ok(false);
    };
    let n = match change {
        ProgressChange::Position {
            key,
            kind,
            position_ms,
            duration_ms,
            ..
        } => conn.execute(
            "INSERT INTO progress (profile_id, media_key, source_type, position_ms, duration_ms,
                                   watched, updated_at_ms, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)
             ON CONFLICT (profile_id, media_key) DO UPDATE SET
                 source_type = excluded.source_type,
                 position_ms = excluded.position_ms,
                 duration_ms = coalesce(excluded.duration_ms, progress.duration_ms),
                 updated_at_ms = excluded.updated_at_ms,
                 device_id = excluded.device_id
             WHERE excluded.updated_at_ms >= progress.updated_at_ms",
            params![
                profile_id,
                check_key(key)?,
                kind_str(*kind),
                to_i64(*position_ms, "position")?,
                duration_ms.map(|d| to_i64(d, "duration")).transpose()?,
                at_ms,
                device_id,
            ],
        )?,
        ProgressChange::Watched { key, .. } => conn.execute(
            "INSERT INTO progress (profile_id, media_key, source_type, position_ms, watched,
                                   updated_at_ms, device_id)
             VALUES (?1, ?2, ?3, 0, 1, ?4, ?5)
             ON CONFLICT (profile_id, media_key) DO UPDATE SET
                 watched = 1,
                 updated_at_ms = max(progress.updated_at_ms, excluded.updated_at_ms),
                 device_id = excluded.device_id
             WHERE progress.watched = 0",
            params![
                profile_id,
                check_key(key)?,
                kind_str(kind_of_key(key)),
                at_ms,
                device_id
            ],
        )?,
        ProgressChange::Unwatched { key, .. } => conn.execute(
            "UPDATE progress SET watched = 0, updated_at_ms = ?3, device_id = ?4
             WHERE profile_id = ?1 AND media_key = ?2 AND watched = 1 AND updated_at_ms <= ?3",
            params![profile_id, check_key(key)?, at_ms, device_id],
        )?,
    };
    Ok(n == 1)
}
