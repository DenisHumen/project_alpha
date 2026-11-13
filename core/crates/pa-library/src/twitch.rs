// SPDX-License-Identifier: MPL-2.0
//! Twitch in the library (TWI-007, docs/research/06 §5): streamers,
//! favourite channels, live sessions, VODs, clips, category segments
//! (chapters), saved files (downloads and recordings), chat logs and record
//! rules. The "Streams" section treats a streamer like a show and a VOD like
//! an episode.
//!
//! **Cache cap.** Metadata from Twitch is a cache of at most 24 hours
//! ([`CACHE_TTL_MS`], Developer Agreement, docs/legal/providers.md §2.4).
//! Every metadata row remembers when it was fetched, and [`purge_expired`]
//! deletes stale rows unless the user saved them:
//!
//! - a streamer stays while a profile has it as a favourite, has a record
//!   rule for it or has a saved file of it;
//! - a VOD, clip or live session stays while a saved file refers to it; a
//!   VOD reported deleted on Twitch goes at once unless it was saved;
//! - a category stays while a remaining chapter or clip refers to it.
//!
//! Progress needs no Twitch metadata: it lives in `progress` under
//! `twitch:vod:<id>` (online and downloaded alike), `twitch:rec:<stream_id>`
//! and `twitch:clip:<slug>` ([`crate::progress`]), so "Continue watching"
//! keeps a VOD after its cached title expired; the UI fetches it again.
//!
//! **Favourites** work without a Twitch login. A change goes through a
//! [`FollowSink`] in the same transaction: `pa-sync` turns it into a
//! `twitch_follow.set` op; ops from other devices are written with
//! [`apply_remote_follow`].
//!
//! ```
//! use pa_library::progress::LocalOnly;
//! use pa_library::twitch::{self, FavoriteSource};
//! use pa_model::{BroadcasterType, Streamer};
//!
//! let mut conn = pa_library::open_in_memory().unwrap();
//! let now = 1_763_000_000_000;
//! let streamer = Streamer {
//!     user_id: "141981764".into(),
//!     login: "twitchdev".into(),
//!     display_name: "TwitchDev".into(),
//!     broadcaster_type: BroadcasterType::Partner,
//!     description: None,
//!     avatar_url: None,
//!     offline_banner_url: None,
//!     last_seen_live_at_ms: None,
//!     updated_at_ms: now,
//! };
//! twitch::upsert_streamer(&conn, &streamer, now).unwrap();
//! let mut sink = LocalOnly::new("phone");
//! twitch::set_favorite(&mut conn, &mut sink, 1, "141981764", FavoriteSource::Local, true, now)
//!     .unwrap();
//! let hits = twitch::search(&conn, "twitchd", 10).unwrap();
//! assert_eq!(hits[0].twitch_id, "141981764");
//! // A day later the favourite keeps its streamer in the library.
//! twitch::purge_expired(&conn, now + twitch::CACHE_TTL_MS + 1).unwrap();
//! assert!(twitch::streamer(&conn, "141981764").unwrap().is_some());
//! ```

use pa_model::{
    BroadcasterType, MediaKey, PlaybackItem, Streamer, Video, VideoAvailability, VideoType,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Error;
use crate::progress::{self, LocalOnly, SinkError};

/// How long Twitch metadata may be cached: 24 hours.
pub const CACHE_TTL_MS: i64 = 24 * 60 * 60 * 1000;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

// --- Enumerations ---------------------------------------------------------------

/// `twitch_streamers.broadcaster_type` (serde names of pa-model).
pub fn broadcaster_type_str(t: BroadcasterType) -> &'static str {
    match t {
        BroadcasterType::Partner => "partner",
        BroadcasterType::Affiliate => "affiliate",
        BroadcasterType::Normal => "normal",
    }
}

fn parse_broadcaster_type(s: &str) -> Result<BroadcasterType, Error> {
    Ok(match s {
        "partner" => BroadcasterType::Partner,
        "affiliate" => BroadcasterType::Affiliate,
        "normal" => BroadcasterType::Normal,
        _ => return Err(Error::Invalid("broadcaster type")),
    })
}

/// `twitch_videos.video_type`.
pub fn video_type_str(t: VideoType) -> &'static str {
    match t {
        VideoType::Archive => "archive",
        VideoType::Highlight => "highlight",
        VideoType::Upload => "upload",
    }
}

fn parse_video_type(s: &str) -> Result<VideoType, Error> {
    Ok(match s {
        "archive" => VideoType::Archive,
        "highlight" => VideoType::Highlight,
        "upload" => VideoType::Upload,
        _ => return Err(Error::Invalid("video type")),
    })
}

/// `twitch_videos.availability`.
pub fn availability_str(a: VideoAvailability) -> &'static str {
    match a {
        VideoAvailability::Online => "online",
        VideoAvailability::Deleted => "deleted",
        VideoAvailability::Unknown => "unknown",
    }
}

fn parse_availability(s: &str) -> Result<VideoAvailability, Error> {
    Ok(match s {
        "online" => VideoAvailability::Online,
        "deleted" => VideoAvailability::Deleted,
        "unknown" => VideoAvailability::Unknown,
        _ => return Err(Error::Invalid("video availability")),
    })
}

/// When Twitch deletes a VOD: archives (past broadcasts) live 7 days for
/// regular channels, 14 for affiliates and 60 for partners; highlights and
/// uploads stay. Drives the "expires soon" filter.
pub fn vod_expires_at_ms(
    video_type: VideoType,
    broadcaster_type: BroadcasterType,
    published_at_ms: i64,
) -> Option<i64> {
    if video_type != VideoType::Archive {
        return None;
    }
    let days = match broadcaster_type {
        BroadcasterType::Partner => 60,
        BroadcasterType::Affiliate => 14,
        BroadcasterType::Normal => 7,
    };
    Some(published_at_ms.saturating_add(days * DAY_MS))
}

fn twitch_id(s: &str, what: &'static str) -> Result<(), Error> {
    let ok = !s.is_empty() && !s.starts_with('0') && s.bytes().all(|b| b.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(what))
    }
}

fn to_i64(v: u64, what: &'static str) -> Result<i64, Error> {
    i64::try_from(v).map_err(|_| Error::Invalid(what))
}

fn opt_i64(v: Option<u64>, what: &'static str) -> Result<Option<i64>, Error> {
    v.map(|v| to_i64(v, what)).transpose()
}

fn opt_u64(v: Option<i64>) -> Option<u64> {
    v.and_then(|v| u64::try_from(v).ok())
}

fn slug(s: &str, what: &'static str) -> Result<(), Error> {
    let ok = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(what))
    }
}

// --- Streamers and categories -----------------------------------------------------

/// Stores a streamer fetched from Twitch at `now_ms`. The local avatar copy
/// ([`set_avatar_local`]) is kept.
pub fn upsert_streamer(conn: &Connection, s: &Streamer, now_ms: i64) -> Result<(), Error> {
    twitch_id(&s.user_id, "user id")?;
    conn.execute(
        "INSERT INTO twitch_streamers (user_id, login, display_name, broadcaster_type, description,
                                       avatar_url, offline_banner_url, last_seen_live_at_ms,
                                       fetched_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT (user_id) DO UPDATE SET
             login = excluded.login,
             display_name = excluded.display_name,
             broadcaster_type = excluded.broadcaster_type,
             description = excluded.description,
             avatar_url = excluded.avatar_url,
             offline_banner_url = excluded.offline_banner_url,
             last_seen_live_at_ms = coalesce(excluded.last_seen_live_at_ms,
                                             twitch_streamers.last_seen_live_at_ms),
             fetched_at_ms = excluded.fetched_at_ms,
             updated_at_ms = excluded.updated_at_ms",
        params![
            s.user_id,
            s.login,
            s.display_name,
            broadcaster_type_str(s.broadcaster_type),
            s.description,
            s.avatar_url,
            s.offline_banner_url,
            s.last_seen_live_at_ms,
            now_ms,
            s.updated_at_ms,
        ],
    )?;
    Ok(())
}

/// Sets or clears the path of the avatar copied to the app cache.
pub fn set_avatar_local(
    conn: &Connection,
    user_id: &str,
    path: Option<&str>,
) -> Result<bool, Error> {
    Ok(conn.execute(
        "UPDATE twitch_streamers SET avatar_local = ?2 WHERE user_id = ?1",
        params![user_id, path],
    )? > 0)
}

const STREAMER_COLUMNS: &str = "user_id, login, display_name, broadcaster_type, description, \
     avatar_url, offline_banner_url, last_seen_live_at_ms, updated_at_ms";

fn streamer_from_row(r: &Row<'_>, at: usize) -> rusqlite::Result<Result<Streamer, Error>> {
    let kind: String = r.get(at + 3)?;
    let s = Streamer {
        user_id: r.get(at)?,
        login: r.get(at + 1)?,
        display_name: r.get(at + 2)?,
        broadcaster_type: BroadcasterType::Normal,
        description: r.get(at + 4)?,
        avatar_url: r.get(at + 5)?,
        offline_banner_url: r.get(at + 6)?,
        last_seen_live_at_ms: r.get(at + 7)?,
        updated_at_ms: r.get(at + 8)?,
    };
    Ok(
        parse_broadcaster_type(&kind).map(|broadcaster_type| Streamer {
            broadcaster_type,
            ..s
        }),
    )
}

/// A cached streamer.
pub fn streamer(conn: &Connection, user_id: &str) -> Result<Option<Streamer>, Error> {
    conn.query_row(
        &format!("SELECT {STREAMER_COLUMNS} FROM twitch_streamers WHERE user_id = ?1"),
        [user_id],
        |r| streamer_from_row(r, 0),
    )
    .optional()?
    .transpose()
}

/// A Twitch category (game).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub game_id: String,
    pub name: String,
    pub box_art_url: Option<String>,
    pub igdb_id: Option<String>,
}

/// Stores a category fetched at `now_ms`.
pub fn upsert_category(conn: &Connection, c: &Category, now_ms: i64) -> Result<(), Error> {
    twitch_id(&c.game_id, "game id")?;
    conn.execute(
        "INSERT INTO twitch_categories (game_id, name, box_art_url, igdb_id, fetched_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (game_id) DO UPDATE SET
             name = excluded.name,
             box_art_url = excluded.box_art_url,
             igdb_id = excluded.igdb_id,
             fetched_at_ms = excluded.fetched_at_ms",
        params![c.game_id, c.name, c.box_art_url, c.igdb_id, now_ms],
    )?;
    Ok(())
}

/// A cached category.
pub fn category(conn: &Connection, game_id: &str) -> Result<Option<Category>, Error> {
    Ok(conn
        .query_row(
            "SELECT game_id, name, box_art_url, igdb_id FROM twitch_categories WHERE game_id = ?1",
            [game_id],
            |r| {
                Ok(Category {
                    game_id: r.get(0)?,
                    name: r.get(1)?,
                    box_art_url: r.get(2)?,
                    igdb_id: r.get(3)?,
                })
            },
        )
        .optional()?)
}

// --- Favourites ---------------------------------------------------------------------

/// Where a favourite came from. Both are synced alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavoriteSource {
    /// Added in the app; no Twitch login needed.
    Local,
    /// Imported from the user's Twitch follows.
    TwitchFollow,
}

impl FavoriteSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::TwitchFollow => "twitch_follow",
        }
    }

    fn parse(s: &str) -> Result<Self, Error> {
        match s {
            "local" => Ok(Self::Local),
            "twitch_follow" => Ok(Self::TwitchFollow),
            _ => Err(Error::Invalid("favorite source")),
        }
    }
}

/// A favourite channel of a profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Favorite {
    pub user_id: String,
    pub source: FavoriteSource,
    /// "X is live" notifications.
    pub notify: bool,
    pub added_at_ms: i64,
    /// Cached metadata, if any.
    pub streamer: Option<Streamer>,
}

/// A local change of favourites, as handed to a [`FollowSink`]; maps onto
/// the pa-sync op `twitch_follow.set`. `profile` is `profiles.sync_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowChange {
    pub profile: String,
    pub user_id: String,
    pub followed: bool,
}

/// Receives local favourite changes inside the transaction that writes
/// them. An error rolls the change back.
pub trait FollowSink {
    fn record_follow(
        &mut self,
        conn: &Connection,
        change: FollowChange,
        now_ms: i64,
    ) -> Result<(), SinkError>;
}

impl FollowSink for LocalOnly {
    fn record_follow(&mut self, _: &Connection, _: FollowChange, _: i64) -> Result<(), SinkError> {
        Ok(())
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

/// Adds (`followed = true`) or removes a favourite channel. Returns false
/// and records nothing if the favourite was already in that state (an
/// existing favourite keeps its source and notification setting).
pub fn set_favorite(
    conn: &mut Connection,
    sink: &mut dyn FollowSink,
    profile_id: i64,
    user_id: &str,
    source: FavoriteSource,
    followed: bool,
    now_ms: i64,
) -> Result<bool, Error> {
    twitch_id(user_id, "user id")?;
    let tx = conn.transaction()?;
    let profile = profile_sync_id(&tx, profile_id)?;
    let changed = if followed {
        tx.execute(
            "INSERT INTO twitch_favorites (profile_id, user_id, source, added_at_ms)
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
            params![profile_id, user_id, source.as_str(), now_ms],
        )?
    } else {
        tx.execute(
            "DELETE FROM twitch_favorites WHERE profile_id = ?1 AND user_id = ?2",
            params![profile_id, user_id],
        )?
    } > 0;
    if changed {
        let change = FollowChange {
            profile,
            user_id: user_id.to_owned(),
            followed,
        };
        sink.record_follow(&tx, change, now_ms)
            .map_err(Error::Sync)?;
    }
    tx.commit()?;
    Ok(changed)
}

/// Turns "X is live" notifications for a favourite on or off. Local to the
/// device.
pub fn set_notify(
    conn: &Connection,
    profile_id: i64,
    user_id: &str,
    notify: bool,
) -> Result<bool, Error> {
    Ok(conn.execute(
        "UPDATE twitch_favorites SET notify = ?3 WHERE profile_id = ?1 AND user_id = ?2",
        params![profile_id, user_id, notify],
    )? > 0)
}

/// Favourites of a profile, by display name (channels without cached
/// metadata last, by id).
pub fn favorites(conn: &Connection, profile_id: i64) -> Result<Vec<Favorite>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT f.user_id, f.source, f.notify, f.added_at_ms, s.id, {}
         FROM twitch_favorites f
         LEFT JOIN twitch_streamers s ON s.user_id = f.user_id
         WHERE f.profile_id = ?1
         ORDER BY s.id IS NULL, s.display_name COLLATE NOCASE, f.user_id",
        prefixed("s", STREAMER_COLUMNS)
    ))?;
    let rows = stmt.query_map([profile_id], |r| {
        let source: String = r.get(1)?;
        let has_streamer: Option<i64> = r.get(4)?;
        let streamer = match has_streamer {
            Some(_) => Some(streamer_from_row(r, 5)?),
            None => None,
        };
        Ok((|| -> Result<Favorite, Error> {
            Ok(Favorite {
                user_id: r.get(0)?,
                source: FavoriteSource::parse(&source)?,
                notify: r.get(2)?,
                added_at_ms: r.get(3)?,
                streamer: streamer.transpose()?,
            })
        })())
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row??);
    }
    Ok(out)
}

fn prefixed(alias: &str, columns: &str) -> String {
    columns
        .split(',')
        .map(|c| format!("{alias}.{}", c.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Writes a favourite change received from another device (after the LWW
/// merge of pa-sync). A new favourite gets source `local`. Returns false if
/// the profile is unknown here, the channel id is not a Twitch id or
/// nothing changed.
pub fn apply_remote_follow(
    conn: &Connection,
    change: &FollowChange,
    at_ms: i64,
) -> Result<bool, Error> {
    if twitch_id(&change.user_id, "user id").is_err() {
        return Ok(false);
    }
    let Some(profile_id): Option<i64> = conn
        .query_row(
            "SELECT id FROM profiles WHERE sync_id = ?1",
            [&change.profile],
            |r| r.get(0),
        )
        .optional()?
    else {
        return Ok(false);
    };
    let n = if change.followed {
        conn.execute(
            "INSERT INTO twitch_favorites (profile_id, user_id, source, added_at_ms)
             VALUES (?1, ?2, 'local', ?3) ON CONFLICT DO NOTHING",
            params![profile_id, change.user_id, at_ms],
        )?
    } else {
        conn.execute(
            "DELETE FROM twitch_favorites WHERE profile_id = ?1 AND user_id = ?2",
            params![profile_id, change.user_id],
        )?
    };
    Ok(n > 0)
}

// --- Live sessions, VODs, chapters, clips -------------------------------------------

/// A broadcast; `video_id` is its archive VOD once known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub stream_id: String,
    pub user_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub title_last: Option<String>,
    pub peak_viewers: Option<u64>,
    pub video_id: Option<String>,
}

/// Stores a live session observed at `now_ms`. The peak keeps the maximum;
/// known end and VOD are not cleared by a later report without them.
pub fn upsert_live_session(conn: &Connection, s: &LiveSession, now_ms: i64) -> Result<(), Error> {
    twitch_id(&s.stream_id, "stream id")?;
    twitch_id(&s.user_id, "user id")?;
    if let Some(v) = &s.video_id {
        twitch_id(v, "video id")?;
    }
    conn.execute(
        "INSERT INTO twitch_live_sessions (stream_id, user_id, started_at_ms, ended_at_ms,
                                           title_last, peak_viewers, video_id, fetched_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (stream_id) DO UPDATE SET
             ended_at_ms = coalesce(excluded.ended_at_ms, twitch_live_sessions.ended_at_ms),
             title_last = coalesce(excluded.title_last, twitch_live_sessions.title_last),
             peak_viewers = max(coalesce(excluded.peak_viewers, 0),
                                coalesce(twitch_live_sessions.peak_viewers, 0)),
             video_id = coalesce(excluded.video_id, twitch_live_sessions.video_id),
             fetched_at_ms = excluded.fetched_at_ms",
        params![
            s.stream_id,
            s.user_id,
            s.started_at_ms,
            s.ended_at_ms,
            s.title_last,
            opt_i64(s.peak_viewers, "peak viewers")?,
            s.video_id,
            now_ms,
        ],
    )?;
    Ok(())
}

/// A cached live session.
pub fn live_session(conn: &Connection, stream_id: &str) -> Result<Option<LiveSession>, Error> {
    Ok(conn
        .query_row(
            "SELECT stream_id, user_id, started_at_ms, ended_at_ms, title_last, peak_viewers, video_id
             FROM twitch_live_sessions WHERE stream_id = ?1",
            [stream_id],
            |r| {
                Ok(LiveSession {
                    stream_id: r.get(0)?,
                    user_id: r.get(1)?,
                    started_at_ms: r.get(2)?,
                    ended_at_ms: r.get(3)?,
                    title_last: r.get(4)?,
                    peak_viewers: opt_u64(r.get(5)?),
                    video_id: r.get(6)?,
                })
            },
        )
        .optional()?)
}

/// Stores a VOD fetched (and so checked) at `now_ms`.
pub fn upsert_video(conn: &Connection, v: &Video, now_ms: i64) -> Result<(), Error> {
    twitch_id(&v.video_id, "video id")?;
    twitch_id(&v.user_id, "user id")?;
    conn.execute(
        "INSERT INTO twitch_videos (video_id, user_id, stream_id, video_type, title, published_at_ms,
                                    duration_ms, view_count, thumbnail_url, is_sub_only,
                                    availability, last_checked_at_ms, fetched_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)
         ON CONFLICT (video_id) DO UPDATE SET
             user_id = excluded.user_id,
             stream_id = coalesce(excluded.stream_id, twitch_videos.stream_id),
             video_type = excluded.video_type,
             title = excluded.title,
             published_at_ms = excluded.published_at_ms,
             duration_ms = excluded.duration_ms,
             view_count = excluded.view_count,
             thumbnail_url = excluded.thumbnail_url,
             is_sub_only = coalesce(excluded.is_sub_only, twitch_videos.is_sub_only),
             availability = excluded.availability,
             last_checked_at_ms = excluded.last_checked_at_ms,
             fetched_at_ms = excluded.fetched_at_ms",
        params![
            v.video_id,
            v.user_id,
            v.stream_id,
            video_type_str(v.video_type),
            v.title,
            v.published_at_ms,
            to_i64(v.duration_ms, "duration")?,
            to_i64(v.view_count, "view count")?,
            v.thumbnail_url,
            v.is_sub_only,
            availability_str(v.availability),
            now_ms,
        ],
    )?;
    Ok(())
}

/// Records the result of checking whether a VOD is still on Twitch. A
/// deleted VOD that was not saved is removed by the next [`purge_expired`].
pub fn set_availability(
    conn: &Connection,
    video_id: &str,
    availability: VideoAvailability,
    now_ms: i64,
) -> Result<bool, Error> {
    Ok(conn.execute(
        "UPDATE twitch_videos SET availability = ?2, last_checked_at_ms = ?3 WHERE video_id = ?1",
        params![video_id, availability_str(availability), now_ms],
    )? > 0)
}

const VIDEO_COLUMNS: &str = "video_id, user_id, stream_id, video_type, title, published_at_ms, \
     duration_ms, view_count, thumbnail_url, is_sub_only, availability";

fn video_from_row(r: &Row<'_>, at: usize) -> rusqlite::Result<Result<Video, Error>> {
    let video_type: String = r.get(at + 3)?;
    let availability: String = r.get(at + 10)?;
    let video_id: String = r.get(at)?;
    let user_id: String = r.get(at + 1)?;
    let stream_id: Option<String> = r.get(at + 2)?;
    let title: String = r.get(at + 4)?;
    let published_at_ms: i64 = r.get(at + 5)?;
    let duration: i64 = r.get(at + 6)?;
    let views: i64 = r.get(at + 7)?;
    let thumbnail_url: Option<String> = r.get(at + 8)?;
    let is_sub_only: Option<bool> = r.get(at + 9)?;
    Ok((|| {
        Ok(Video {
            video_id,
            user_id,
            stream_id,
            video_type: parse_video_type(&video_type)?,
            title,
            published_at_ms,
            duration_ms: u64::try_from(duration).unwrap_or(0),
            view_count: u64::try_from(views).unwrap_or(0),
            thumbnail_url,
            is_sub_only,
            availability: parse_availability(&availability)?,
        })
    })())
}

/// A cached VOD.
pub fn video(conn: &Connection, video_id: &str) -> Result<Option<Video>, Error> {
    conn.query_row(
        &format!("SELECT {VIDEO_COLUMNS} FROM twitch_videos WHERE video_id = ?1"),
        [video_id],
        |r| video_from_row(r, 0),
    )
    .optional()?
    .transpose()
}

/// What a chapter belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentOwner {
    /// A VOD; offsets are from its start.
    Video(String),
    /// A live session; offsets are from its start.
    Stream(String),
}

/// A chapter: the category on air from `start_ms` to `end_ms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorySegment {
    pub game_id: String,
    pub title_at_time: Option<String>,
    pub start_ms: u64,
    /// `None` while still on air.
    pub end_ms: Option<u64>,
    /// `eventsub`, `gql_moments`, `fallback`… (lowercase slug).
    pub source: String,
}

/// Replaces the chapters of a VOD or live session (which must be stored).
pub fn replace_segments(
    conn: &mut Connection,
    owner: &SegmentOwner,
    segments: &[CategorySegment],
) -> Result<(), Error> {
    let (video_id, stream_id) = match owner {
        SegmentOwner::Video(v) => (Some(v.as_str()), None),
        SegmentOwner::Stream(s) => (None, Some(s.as_str())),
    };
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM twitch_category_segments WHERE video_id IS ?1 AND stream_id IS ?2",
        params![video_id, stream_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO twitch_category_segments (video_id, stream_id, game_id, title_at_time,
                                                   start_ms, end_ms, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for s in segments {
            slug(&s.source, "segment source")?;
            stmt.execute(params![
                video_id,
                stream_id,
                s.game_id,
                s.title_at_time,
                to_i64(s.start_ms, "segment start")?,
                opt_i64(s.end_ms, "segment end")?,
                s.source,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Chapters of a VOD or live session, by start.
pub fn segments(conn: &Connection, owner: &SegmentOwner) -> Result<Vec<CategorySegment>, Error> {
    let (video_id, stream_id) = match owner {
        SegmentOwner::Video(v) => (Some(v.as_str()), None),
        SegmentOwner::Stream(s) => (None, Some(s.as_str())),
    };
    let mut stmt = conn.prepare(
        "SELECT game_id, title_at_time, start_ms, end_ms, source FROM twitch_category_segments
         WHERE video_id IS ?1 AND stream_id IS ?2 ORDER BY start_ms, id",
    )?;
    let rows = stmt.query_map(params![video_id, stream_id], |r| {
        Ok(CategorySegment {
            game_id: r.get(0)?,
            title_at_time: r.get(1)?,
            start_ms: opt_u64(r.get(2)?).unwrap_or(0),
            end_ms: opt_u64(r.get(3)?),
            source: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A Twitch clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub slug: String,
    pub broadcaster_id: String,
    pub creator_name: Option<String>,
    pub video_id: Option<String>,
    pub vod_offset_ms: Option<u64>,
    pub game_id: Option<String>,
    pub title: String,
    pub created_at_ms: i64,
    pub duration_ms: u64,
    pub thumbnail_url: Option<String>,
    pub view_count: u64,
}

impl Clip {
    /// `twitch:clip:<slug>`.
    pub fn media_key(&self) -> MediaKey {
        MediaKey::TwitchClip {
            slug: self.slug.clone(),
        }
    }
}

/// Stores a clip fetched at `now_ms`.
pub fn upsert_clip(conn: &Connection, c: &Clip, now_ms: i64) -> Result<(), Error> {
    if !c.media_key().is_valid() {
        return Err(Error::Invalid("clip slug"));
    }
    twitch_id(&c.broadcaster_id, "broadcaster id")?;
    conn.execute(
        "INSERT INTO twitch_clips (slug, broadcaster_id, creator_name, video_id, vod_offset_ms,
                                   game_id, title, created_at_ms, duration_ms, thumbnail_url,
                                   view_count, fetched_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (slug) DO UPDATE SET
             broadcaster_id = excluded.broadcaster_id,
             creator_name = excluded.creator_name,
             video_id = excluded.video_id,
             vod_offset_ms = excluded.vod_offset_ms,
             game_id = excluded.game_id,
             title = excluded.title,
             created_at_ms = excluded.created_at_ms,
             duration_ms = excluded.duration_ms,
             thumbnail_url = excluded.thumbnail_url,
             view_count = excluded.view_count,
             fetched_at_ms = excluded.fetched_at_ms",
        params![
            c.slug,
            c.broadcaster_id,
            c.creator_name,
            c.video_id,
            opt_i64(c.vod_offset_ms, "vod offset")?,
            c.game_id,
            c.title,
            c.created_at_ms,
            to_i64(c.duration_ms, "duration")?,
            c.thumbnail_url,
            to_i64(c.view_count, "view count")?,
            now_ms,
        ],
    )?;
    Ok(())
}

/// A cached clip.
pub fn clip(conn: &Connection, slug: &str) -> Result<Option<Clip>, Error> {
    Ok(conn
        .query_row(
            "SELECT slug, broadcaster_id, creator_name, video_id, vod_offset_ms, game_id, title,
                    created_at_ms, duration_ms, thumbnail_url, view_count
             FROM twitch_clips WHERE slug = ?1",
            [slug],
            |r| {
                Ok(Clip {
                    slug: r.get(0)?,
                    broadcaster_id: r.get(1)?,
                    creator_name: r.get(2)?,
                    video_id: r.get(3)?,
                    vod_offset_ms: opt_u64(r.get(4)?),
                    game_id: r.get(5)?,
                    title: r.get(6)?,
                    created_at_ms: r.get(7)?,
                    duration_ms: opt_u64(r.get(8)?).unwrap_or(0),
                    thumbnail_url: r.get(9)?,
                    view_count: opt_u64(r.get(10)?).unwrap_or(0),
                })
            },
        )
        .optional()?)
}

// --- Saved files ----------------------------------------------------------------------

/// What a saved Twitch file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// A downloaded VOD (`twitch:vod:<video_id>`).
    Vod,
    /// Our own live recording (`twitch:rec:<stream_id>`).
    Recording,
    /// A downloaded clip (`twitch:clip:<slug>`).
    Clip,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vod => "vod",
            Self::Recording => "recording",
            Self::Clip => "clip",
        }
    }

    fn parse(s: &str) -> Result<Self, Error> {
        match s {
            "vod" => Ok(Self::Vod),
            "recording" => Ok(Self::Recording),
            "clip" => Ok(Self::Clip),
            _ => Err(Error::Invalid("asset kind")),
        }
    }
}

/// A saved Twitch file: a row of `files` plus what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// `twitch_assets.id`; 0 in [`add_asset`] input.
    pub id: i64,
    /// `files.id`.
    pub file_id: i64,
    pub kind: AssetKind,
    /// The streamer.
    pub user_id: String,
    /// The VOD; for a recording, set once linked ([`link_recording`]).
    pub video_id: Option<String>,
    pub stream_id: Option<String>,
    pub clip_slug: Option<String>,
    /// Title as saved with the file.
    pub title: String,
    pub recorded_at_ms: Option<i64>,
    pub duration_ms: Option<u64>,
    pub quality: Option<String>,
    pub has_ads_marked: bool,
    /// Recording → VOD: VOD position = recording position + offset.
    pub offset_rec_to_vod_ms: Option<i64>,
    /// `server`, `desktop`, `import`…
    pub origin: String,
    pub created_at_ms: i64,
}

impl Asset {
    /// The sync key of the file: a downloaded VOD shares `twitch:vod:<id>`
    /// with the online VOD; a recording keeps `twitch:rec:<stream_id>`.
    pub fn media_key(&self) -> Option<MediaKey> {
        let key = match self.kind {
            AssetKind::Vod => MediaKey::TwitchVod {
                video_id: self.video_id.clone()?,
            },
            AssetKind::Recording => MediaKey::TwitchRecording {
                stream_id: self.stream_id.clone()?,
            },
            AssetKind::Clip => MediaKey::TwitchClip {
                slug: self.clip_slug.clone()?,
            },
        };
        key.is_valid().then_some(key)
    }
}

/// Registers a saved file (download finished, recording started or an
/// imported file). Returns `twitch_assets.id`. `id` and `created_at_ms` of
/// the input are ignored.
pub fn add_asset(conn: &Connection, a: &Asset, now_ms: i64) -> Result<i64, Error> {
    twitch_id(&a.user_id, "user id")?;
    slug(&a.origin, "asset origin")?;
    if a.media_key().is_none() {
        return Err(Error::Invalid("asset key"));
    }
    if a.offset_rec_to_vod_ms.is_some() && (a.kind != AssetKind::Recording || a.video_id.is_none())
    {
        return Err(Error::Invalid("recording offset"));
    }
    conn.execute(
        "INSERT INTO twitch_assets (file_id, kind, user_id, video_id, stream_id, clip_slug, title,
                                    recorded_at_ms, duration_ms, quality, has_ads_marked,
                                    offset_rec_to_vod_ms, origin, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            a.file_id,
            a.kind.as_str(),
            a.user_id,
            a.video_id,
            a.stream_id,
            a.clip_slug,
            a.title,
            a.recorded_at_ms,
            opt_i64(a.duration_ms, "duration")?,
            a.quality,
            a.has_ads_marked,
            a.offset_rec_to_vod_ms,
            a.origin,
            now_ms,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Links the recordings of a broadcast to its VOD once Twitch published it:
/// VOD position = recording position + `offset_ms`. Also fills
/// `twitch_live_sessions.video_id`. Returns the number of linked files.
pub fn link_recording(
    conn: &mut Connection,
    stream_id: &str,
    video_id: &str,
    offset_ms: i64,
) -> Result<usize, Error> {
    twitch_id(stream_id, "stream id")?;
    twitch_id(video_id, "video id")?;
    let tx = conn.transaction()?;
    let n = tx.execute(
        "UPDATE twitch_assets SET video_id = ?2, offset_rec_to_vod_ms = ?3
         WHERE kind = 'recording' AND stream_id = ?1",
        params![stream_id, video_id, offset_ms],
    )?;
    tx.execute(
        "UPDATE twitch_live_sessions SET video_id = ?2 WHERE stream_id = ?1",
        params![stream_id, video_id],
    )?;
    tx.commit()?;
    Ok(n)
}

const ASSET_COLUMNS: &str = "id, file_id, kind, user_id, video_id, stream_id, clip_slug, title, \
     recorded_at_ms, duration_ms, quality, has_ads_marked, offset_rec_to_vod_ms, origin, created_at_ms";

fn asset_from_row(r: &Row<'_>) -> rusqlite::Result<Result<Asset, Error>> {
    let kind: String = r.get(2)?;
    let a = Asset {
        id: r.get(0)?,
        file_id: r.get(1)?,
        kind: AssetKind::Vod,
        user_id: r.get(3)?,
        video_id: r.get(4)?,
        stream_id: r.get(5)?,
        clip_slug: r.get(6)?,
        title: r.get(7)?,
        recorded_at_ms: r.get(8)?,
        duration_ms: opt_u64(r.get(9)?),
        quality: r.get(10)?,
        has_ads_marked: r.get(11)?,
        offset_rec_to_vod_ms: r.get(12)?,
        origin: r.get(13)?,
        created_at_ms: r.get(14)?,
    };
    Ok(AssetKind::parse(&kind).map(|kind| Asset { kind, ..a }))
}

fn collect_assets(
    conn: &Connection,
    filter: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<Asset>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ASSET_COLUMNS} FROM twitch_assets WHERE {filter}"
    ))?;
    let rows = stmt.query_map(args, asset_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row??);
    }
    Ok(out)
}

/// Saved files that can play `key`: for `twitch:vod:` downloaded copies and
/// recordings linked to the VOD (their offset maps positions), for
/// `twitch:rec:` the recordings, for `twitch:clip:` the clip files. Order:
/// downloaded copies first, then by age.
pub fn assets_for_key(conn: &Connection, key: &MediaKey) -> Result<Vec<Asset>, Error> {
    match key {
        MediaKey::TwitchVod { video_id } => collect_assets(
            conn,
            "video_id = ?1 ORDER BY kind <> 'vod', created_at_ms, id",
            [video_id],
        ),
        MediaKey::TwitchRecording { stream_id } => collect_assets(
            conn,
            "stream_id = ?1 AND kind = 'recording' ORDER BY created_at_ms, id",
            [stream_id],
        ),
        MediaKey::TwitchClip { slug } => collect_assets(
            conn,
            "clip_slug = ?1 AND kind = 'clip' ORDER BY created_at_ms, id",
            [slug],
        ),
        _ => Ok(Vec::new()),
    }
}

/// Saved files of a channel, newest first.
pub fn channel_assets(conn: &Connection, user_id: &str) -> Result<Vec<Asset>, Error> {
    collect_assets(
        conn,
        "user_id = ?1 ORDER BY coalesce(recorded_at_ms, created_at_ms) DESC, id DESC",
        [user_id],
    )
}

/// The chat sidecar of a saved file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLog {
    /// Relative to the root of the source of the asset's file.
    pub path: String,
    /// `tdl_json_v1` (TwitchDownloader JSON).
    pub format: String,
    pub message_count: u64,
    pub first_offset_ms: Option<u64>,
    pub last_offset_ms: Option<u64>,
    /// Emotes were saved with the log.
    pub emote_snapshot: bool,
}

/// Sets (replaces) the chat log of a saved file.
pub fn set_chat_log(
    conn: &Connection,
    asset_id: i64,
    log: &ChatLog,
    now_ms: i64,
) -> Result<(), Error> {
    slug(&log.format, "chat log format")?;
    conn.execute(
        "INSERT INTO twitch_chat_logs (asset_id, path, format, message_count, first_offset_ms,
                                       last_offset_ms, emote_snapshot, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (asset_id) DO UPDATE SET
             path = excluded.path,
             format = excluded.format,
             message_count = excluded.message_count,
             first_offset_ms = excluded.first_offset_ms,
             last_offset_ms = excluded.last_offset_ms,
             emote_snapshot = excluded.emote_snapshot",
        params![
            asset_id,
            log.path,
            log.format,
            to_i64(log.message_count, "message count")?,
            opt_i64(log.first_offset_ms, "chat offset")?,
            opt_i64(log.last_offset_ms, "chat offset")?,
            log.emote_snapshot,
            now_ms,
        ],
    )?;
    Ok(())
}

/// The chat log of a saved file.
pub fn chat_log(conn: &Connection, asset_id: i64) -> Result<Option<ChatLog>, Error> {
    Ok(conn
        .query_row(
            "SELECT path, format, message_count, first_offset_ms, last_offset_ms, emote_snapshot
             FROM twitch_chat_logs WHERE asset_id = ?1",
            [asset_id],
            |r| {
                Ok(ChatLog {
                    path: r.get(0)?,
                    format: r.get(1)?,
                    message_count: opt_u64(r.get(2)?).unwrap_or(0),
                    first_offset_ms: opt_u64(r.get(3)?),
                    last_offset_ms: opt_u64(r.get(4)?),
                    emote_snapshot: r.get(5)?,
                })
            },
        )
        .optional()?)
}

// --- Record rules ---------------------------------------------------------------------

/// Automatic recording of a channel for a profile (full builds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordRule {
    pub profile_id: i64,
    pub user_id: String,
    /// `best`, `1080p60`, `audio_only`…
    pub quality: String,
    pub with_chat: bool,
    pub retention_days: Option<u32>,
    pub keep_last_n: Option<u32>,
    /// Record only these categories (game ids); `None` records all.
    pub categories: Option<Vec<String>>,
    /// Replace the recording with the VOD once it is published.
    pub replace_with_vod: bool,
    pub enabled: bool,
}

fn categories_json(c: &Option<Vec<String>>) -> Result<Option<String>, Error> {
    let Some(ids) = c else { return Ok(None) };
    for id in ids {
        twitch_id(id, "game id")?;
    }
    // Ids are decimal digits: no escaping needed.
    Ok(Some(format!(
        "[{}]",
        ids.iter()
            .map(|i| format!("\"{i}\""))
            .collect::<Vec<_>>()
            .join(",")
    )))
}

/// Creates or replaces the rule of a profile for a channel. Returns its id.
pub fn upsert_record_rule(conn: &Connection, rule: &RecordRule, now_ms: i64) -> Result<i64, Error> {
    twitch_id(&rule.user_id, "user id")?;
    if rule.quality.is_empty() {
        return Err(Error::Invalid("quality"));
    }
    Ok(conn.query_row(
        "INSERT INTO twitch_record_rules (profile_id, user_id, quality, with_chat, retention_days,
                                          keep_last_n, categories_filter_json, replace_with_vod,
                                          enabled, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT (profile_id, user_id) DO UPDATE SET
             quality = excluded.quality,
             with_chat = excluded.with_chat,
             retention_days = excluded.retention_days,
             keep_last_n = excluded.keep_last_n,
             categories_filter_json = excluded.categories_filter_json,
             replace_with_vod = excluded.replace_with_vod,
             enabled = excluded.enabled,
             updated_at_ms = excluded.updated_at_ms
         RETURNING id",
        params![
            rule.profile_id,
            rule.user_id,
            rule.quality,
            rule.with_chat,
            rule.retention_days,
            rule.keep_last_n,
            categories_json(&rule.categories)?,
            rule.replace_with_vod,
            rule.enabled,
            now_ms,
        ],
        |r| r.get(0),
    )?)
}

/// Deletes the rule of a profile for a channel.
pub fn delete_record_rule(
    conn: &Connection,
    profile_id: i64,
    user_id: &str,
) -> Result<bool, Error> {
    Ok(conn.execute(
        "DELETE FROM twitch_record_rules WHERE profile_id = ?1 AND user_id = ?2",
        params![profile_id, user_id],
    )? > 0)
}

/// Record rules; `profile_id: None` returns those of every profile (the
/// recorder serves the whole household).
pub fn record_rules(conn: &Connection, profile_id: Option<i64>) -> Result<Vec<RecordRule>, Error> {
    let mut stmt = conn.prepare(
        "SELECT profile_id, user_id, quality, with_chat, retention_days, keep_last_n,
                categories_filter_json, replace_with_vod, enabled
         FROM twitch_record_rules
         WHERE ?1 IS NULL OR profile_id = ?1
         ORDER BY profile_id, user_id",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        let filter: Option<String> = r.get(6)?;
        // Written by `categories_json`: an array of decimal ids.
        let categories = filter.map(|json| {
            json.split(|c: char| !c.is_ascii_digit())
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        });
        Ok(RecordRule {
            profile_id: r.get(0)?,
            user_id: r.get(1)?,
            quality: r.get(2)?,
            with_chat: r.get(3)?,
            retention_days: r.get(4)?,
            keep_last_n: r.get(5)?,
            categories,
            replace_with_vod: r.get(7)?,
            enabled: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// --- The "Streams" section ----------------------------------------------------------

/// A channel of the "Streams" section (like a show).
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub user_id: String,
    /// Cached metadata, if any.
    pub streamer: Option<Streamer>,
    /// A favourite of the profile.
    pub favorite: bool,
    /// Saved files of the channel.
    pub saved_files: u64,
}

/// Channels of the section: the profile's favourites and channels with
/// saved files, by display name.
pub fn channels(conn: &Connection, profile_id: i64) -> Result<Vec<Channel>, Error> {
    let mut stmt = conn.prepare(&format!(
        "WITH ids (user_id) AS (
             SELECT user_id FROM twitch_favorites WHERE profile_id = ?1
             UNION SELECT user_id FROM twitch_assets)
         SELECT ids.user_id,
                EXISTS (SELECT 1 FROM twitch_favorites f
                        WHERE f.profile_id = ?1 AND f.user_id = ids.user_id),
                (SELECT count(*) FROM twitch_assets a WHERE a.user_id = ids.user_id),
                s.id, {}
         FROM ids LEFT JOIN twitch_streamers s ON s.user_id = ids.user_id
         ORDER BY s.id IS NULL, s.display_name COLLATE NOCASE, ids.user_id",
        prefixed("s", STREAMER_COLUMNS)
    ))?;
    let rows = stmt.query_map([profile_id], |r| {
        let has_streamer: Option<i64> = r.get(3)?;
        let streamer = match has_streamer {
            Some(_) => Some(streamer_from_row(r, 4)?),
            None => None,
        };
        Ok((|| -> Result<Channel, Error> {
            Ok(Channel {
                user_id: r.get(0)?,
                favorite: r.get(1)?,
                saved_files: opt_u64(r.get(2)?).unwrap_or(0),
                streamer: streamer.transpose()?,
            })
        })())
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row??);
    }
    Ok(out)
}

/// Options of [`channel_videos`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VideoQuery {
    /// Only VODs with a chapter in this category.
    pub game_id: Option<String>,
    /// Only VODs with a saved file.
    pub saved_only: bool,
    /// Only VODs Twitch deletes before this time (Unix ms): "expires soon".
    pub expires_before_ms: Option<i64>,
    /// 0 = no limit.
    pub limit: usize,
}

/// A VOD of a channel (like an episode).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoEntry {
    pub video: Video,
    /// Saved files that play it (downloads and linked recordings).
    pub saved_files: u64,
    /// The profile's playback state under `twitch:vod:<id>`.
    pub progress: Option<PlaybackItem>,
    /// When Twitch deletes it ([`vod_expires_at_ms`]); `None` for
    /// highlights and uploads.
    pub expires_at_ms: Option<i64>,
}

/// VODs of a channel, newest first, with saved files, progress and expiry.
pub fn channel_videos(
    conn: &Connection,
    profile_id: i64,
    user_id: &str,
    q: &VideoQuery,
) -> Result<Vec<VideoEntry>, Error> {
    let broadcaster_type = match streamer(conn, user_id)? {
        Some(s) => s.broadcaster_type,
        None => BroadcasterType::Normal,
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, (SELECT count(*) FROM twitch_assets a WHERE a.video_id = v.video_id)
         FROM twitch_videos v
         WHERE v.user_id = ?1
           AND (?2 IS NULL OR EXISTS (SELECT 1 FROM twitch_category_segments g
                                      WHERE g.video_id = v.video_id AND g.game_id = ?2))
           AND (NOT ?3 OR EXISTS (SELECT 1 FROM twitch_assets a WHERE a.video_id = v.video_id))
         ORDER BY v.published_at_ms DESC, v.id DESC",
        prefixed("v", VIDEO_COLUMNS)
    ))?;
    let rows = stmt.query_map(params![user_id, q.game_id, q.saved_only], |r| {
        let saved: i64 = r.get(11)?;
        Ok(video_from_row(r, 0)?.map(|v| (v, saved)))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (video, saved) = row??;
        let expires_at_ms =
            vod_expires_at_ms(video.video_type, broadcaster_type, video.published_at_ms);
        if let Some(before) = q.expires_before_ms
            && expires_at_ms.is_none_or(|e| e >= before)
        {
            continue;
        }
        let progress = progress::get(conn, profile_id, &video.media_key())?;
        out.push(VideoEntry {
            video,
            saved_files: u64::try_from(saved).unwrap_or(0),
            progress,
            expires_at_ms,
        });
        if q.limit > 0 && out.len() >= q.limit {
            break;
        }
    }
    Ok(out)
}

// --- Search ------------------------------------------------------------------------

/// What a search hit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    Streamer,
    Video,
    Category,
    Clip,
    LiveSession,
}

/// A full-text search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub kind: SearchKind,
    /// `user_id`, `video_id`, `game_id`, clip slug or `stream_id`.
    pub twitch_id: String,
    /// The indexed text (name or title).
    pub text: String,
}

/// FTS5 query: every word must match as a prefix. Quotes make each word a
/// literal, so user input cannot use FTS5 syntax.
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Searches streamers (display name, login), VOD, clip and live titles and
/// categories, best match first.
pub fn search(conn: &Connection, input: &str, limit: usize) -> Result<Vec<SearchHit>, Error> {
    let Some(query) = fts_query(input) else {
        return Ok(Vec::new());
    };
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(
        "SELECT s.rowid % 8, s.text,
                CASE s.rowid % 8
                    WHEN 0 THEN (SELECT user_id FROM twitch_streamers WHERE id = s.rowid / 8)
                    WHEN 1 THEN (SELECT video_id FROM twitch_videos WHERE id = s.rowid / 8)
                    WHEN 2 THEN (SELECT game_id FROM twitch_categories WHERE id = s.rowid / 8)
                    WHEN 3 THEN (SELECT slug FROM twitch_clips WHERE id = s.rowid / 8)
                    WHEN 4 THEN (SELECT stream_id FROM twitch_live_sessions WHERE id = s.rowid / 8)
                END
         FROM twitch_search s
         WHERE twitch_search MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, limit], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, text, id) = row?;
        let kind = match kind {
            0 => SearchKind::Streamer,
            1 => SearchKind::Video,
            2 => SearchKind::Category,
            3 => SearchKind::Clip,
            4 => SearchKind::LiveSession,
            _ => continue,
        };
        if let Some(twitch_id) = id {
            out.push(SearchHit {
                kind,
                twitch_id,
                text,
            });
        }
    }
    Ok(out)
}

// --- Cache cap -----------------------------------------------------------------------

/// Rows removed by [`purge_expired`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PurgeReport {
    pub videos: usize,
    pub live_sessions: usize,
    pub clips: usize,
    pub categories: usize,
    pub streamers: usize,
}

/// Deletes Twitch metadata fetched more than [`CACHE_TTL_MS`] before
/// `now_ms`, except what the user saved (see the module docs), and VODs
/// deleted on Twitch that were not saved. Chapters go with their VOD or
/// live session. Run it at start and periodically (e.g. hourly).
pub fn purge_expired(conn: &Connection, now_ms: i64) -> Result<PurgeReport, Error> {
    let cut = now_ms.saturating_sub(CACHE_TTL_MS);
    let tx = conn.unchecked_transaction()?;
    let videos = tx.execute(
        "DELETE FROM twitch_videos
         WHERE (fetched_at_ms < ?1 OR availability = 'deleted')
           AND video_id NOT IN (SELECT video_id FROM twitch_assets WHERE video_id IS NOT NULL)",
        [cut],
    )?;
    let live_sessions = tx.execute(
        "DELETE FROM twitch_live_sessions
         WHERE fetched_at_ms < ?1
           AND stream_id NOT IN (SELECT stream_id FROM twitch_assets WHERE stream_id IS NOT NULL)",
        [cut],
    )?;
    let clips = tx.execute(
        "DELETE FROM twitch_clips
         WHERE fetched_at_ms < ?1
           AND slug NOT IN (SELECT clip_slug FROM twitch_assets WHERE clip_slug IS NOT NULL)",
        [cut],
    )?;
    let categories = tx.execute(
        "DELETE FROM twitch_categories
         WHERE fetched_at_ms < ?1
           AND game_id NOT IN (SELECT game_id FROM twitch_category_segments)
           AND game_id NOT IN (SELECT game_id FROM twitch_clips WHERE game_id IS NOT NULL)",
        [cut],
    )?;
    let streamers = tx.execute(
        "DELETE FROM twitch_streamers
         WHERE fetched_at_ms < ?1
           AND user_id NOT IN (SELECT user_id FROM twitch_favorites)
           AND user_id NOT IN (SELECT user_id FROM twitch_assets)
           AND user_id NOT IN (SELECT user_id FROM twitch_record_rules)",
        [cut],
    )?;
    tx.commit()?;
    Ok(PurgeReport {
        videos,
        live_sessions,
        clips,
        categories,
        streamers,
    })
}
