// SPDX-License-Identifier: MPL-2.0
//! What the Detail screen shows beyond the item row (UI-009): genres,
//! clearlogo, external ratings and ids, cast and crew, the extras next to
//! the file and the probe data of the version the item plays.
//!
//! Everything comes from the library; nothing here asks a provider. Cast,
//! crew and ratings are written by the metadata run (`metadata`), the
//! clearlogo and extras by the folder analysis (LIB-007), the technical
//! data by the probe (LIB-006).

use pa_library::rusqlite::{Connection, OptionalExtension, params};
use pa_library::views;
use serde_json::Value;

use crate::items::{self, ItemRef};
use crate::lists;
use crate::{Catalog, Error};

/// Details of a movie or show item.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemDetails {
    pub genres: Vec<String>,
    /// Runtime from the provider (movies), else `None`.
    pub runtime_ms: Option<i64>,
    /// Provider logo (`https://…`) of the work, if one was fetched.
    pub logo_url: Option<String>,
    /// Local clearlogo next to the files, relative to the source root.
    pub local_logo: Option<FileRef>,
    pub ratings: Vec<Rating>,
    pub external_ids: Vec<ExternalId>,
    /// Cast first (billing order), then crew.
    pub credits: Vec<Credit>,
    /// Extras of the folder of the version the item plays (movies).
    pub extras: Vec<Extra>,
    /// Probe data of the version the item plays (movies).
    pub tech: Option<TechInfo>,
}

/// A file of a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub source_id: i64,
    /// Path relative to the source root.
    pub rel_path: String,
    /// Absolute local path for `file://` sources.
    pub local_path: Option<String>,
}

/// An average user rating, 0–10.
#[derive(Debug, Clone, PartialEq)]
pub struct Rating {
    /// `tmdb`, …
    pub provider: String,
    pub value: f64,
    pub votes: Option<i64>,
}

/// An id of the work at a provider (`tmdb` `movie` `603`, `imdb` `tt0133093`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalId {
    pub provider: String,
    /// Entity type inside the provider; empty if none.
    pub kind: String,
    pub id: String,
}

/// A person of the cast or crew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    /// Crew member (`job` set) rather than an actor (`character` set).
    pub crew: bool,
    pub name: String,
    pub character: Option<String>,
    pub job: Option<String>,
    /// `tmdb:<person id>` for people pages.
    pub person_ref: Option<String>,
    pub profile_url: Option<String>,
}

/// A trailer, featurette… next to the main file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    pub file: FileRef,
    /// `trailer`, `teaser`, `featurette`, `behind_the_scenes`, … (LIB-007),
    /// `other` if unknown.
    pub extra_type: String,
    /// File name without the extension.
    pub title: String,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
}

/// What the probe found in a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TechInfo {
    pub file: Option<FileRef>,
    /// libavformat demuxer (`matroska,webm`, `mov,mp4,…`).
    pub container: Option<String>,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
    /// FFmpeg codec name of the main video stream (`hevc`, `h264`, `av1`).
    pub video_codec: Option<String>,
    pub video_profile: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub bit_depth: Option<i64>,
    /// Frames per second.
    pub frame_rate: Option<f64>,
    /// HDR formats, strongest first: `dolby_vision`, `hdr10_plus`, `hdr10`, `hlg`.
    pub hdr: Vec<String>,
    /// Audio format slugs in stream order (`eac3`, `atmos`, `dts_hd_ma`…).
    pub audio_formats: Vec<String>,
    /// ISO 639-2 languages of the audio streams.
    pub audio_langs: Vec<String>,
    /// ISO 639-2 languages of the subtitle streams.
    pub sub_langs: Vec<String>,
}

fn json_strings(v: Option<String>) -> Vec<String> {
    v.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s.to_owned(),
        _ => name.to_owned(),
    }
}

fn file_ref(uri: &str, path: &str, source_id: i64) -> FileRef {
    let rel = items::relative(path);
    FileRef {
        source_id,
        local_path: items::local_path(uri, &rel),
        rel_path: rel,
    }
}

fn tech(conn: &Connection, file_id: i64) -> Result<Option<TechInfo>, Error> {
    let row = conn
        .query_row(
            "SELECT f.source_id, s.uri, f.path, f.container, f.size_bytes, f.duration_ms,
                    f.video_json, f.audio_formats, f.audio_langs, f.sub_langs
             FROM files f JOIN sources s ON s.id = f.source_id WHERE f.id = ?1",
            [file_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                    r.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((source_id, uri, path, container, size, duration, video, audio, alangs, slangs)) =
        row
    else {
        return Ok(None);
    };
    let video: Value = video
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or(Value::Null);
    let text = |k: &str| {
        video
            .get(k)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty())
    };
    let int = |k: &str| video.get(k).and_then(Value::as_i64).filter(|v| *v > 0);
    let frame_rate = video
        .get("frame_rate")
        .and_then(Value::as_array)
        .and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
        .filter(|(_, d)| *d > 0.0)
        .map(|(n, d)| n / d);
    let hdr = video
        .get("hdr")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(TechInfo {
        file: Some(file_ref(&uri, &path, source_id)),
        container: container.filter(|c| !c.is_empty()),
        size_bytes: size,
        duration_ms: duration,
        video_codec: text("codec"),
        video_profile: text("profile"),
        width: int("width"),
        height: int("height"),
        bit_depth: int("bit_depth"),
        frame_rate,
        hdr,
        audio_formats: json_strings(audio),
        audio_langs: json_strings(alangs),
        sub_langs: json_strings(slangs),
    }))
}

/// Extras and the clearlogo of the folder group of `file_id`.
fn group_assets(
    conn: &Connection,
    file_id: i64,
) -> Result<(Vec<Extra>, Option<FileRef>), Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT g.source_id, s.uri, ga.path, ga.role, ga.extra_type, ga.art_kind,
                ga.season_number, coalesce(f.size_bytes, ga.size_bytes), f.duration_ms
         FROM file_group_members me
         JOIN file_groups g ON g.id = me.group_id
         JOIN sources s ON s.id = g.source_id
         JOIN group_assets ga ON ga.group_id = g.id
         LEFT JOIN files f ON f.id = ga.file_id
         WHERE me.file_id = ?1 AND ga.role IN ('extra', 'art')
         UNION ALL
         SELECT g.source_id, s.uri, f.path, 'extra', gm.extra_type, NULL, NULL,
                f.size_bytes, f.duration_ms
         FROM file_group_members me
         JOIN file_groups g ON g.id = me.group_id
         JOIN sources s ON s.id = g.source_id
         JOIN file_group_members gm ON gm.group_id = g.id AND gm.role = 'extra'
         JOIN files f ON f.id = gm.file_id AND f.missing_since_ms IS NULL
         WHERE me.file_id = ?1",
    )?;
    let rows = stmt.query_map(params![file_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, Option<i64>>(8)?,
        ))
    })?;
    let mut extras: Vec<Extra> = Vec::new();
    let mut logo = None;
    for row in rows {
        let (source_id, uri, path, role, extra_type, art, season, size, duration) = row?;
        if role == "art" {
            if art.as_deref() == Some("clearlogo") && season.is_none() && logo.is_none() {
                logo = Some(file_ref(&uri, &path, source_id));
            }
            continue;
        }
        let file = file_ref(&uri, &path, source_id);
        if extras.iter().any(|e| e.file.rel_path == file.rel_path) {
            continue;
        }
        extras.push(Extra {
            title: stem(&file.rel_path),
            file,
            extra_type: extra_type
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "other".to_owned()),
            size_bytes: size,
            duration_ms: duration,
        });
    }
    // Trailers first, then by name.
    let order = |t: &str| match t {
        "trailer" => 0,
        "teaser" => 1,
        _ => 2,
    };
    extras.sort_by(|a, b| {
        order(&a.extra_type)
            .cmp(&order(&b.extra_type))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    Ok((extras, logo))
}

fn work_details(conn: &Connection, work: i64, out: &mut ItemDetails) -> Result<(), Error> {
    out.genres = views::genres(conn, work)?;
    out.runtime_ms = conn
        .query_row("SELECT runtime_ms FROM works WHERE id = ?1", [work], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    out.logo_url = conn
        .query_row(
            "SELECT url FROM artwork WHERE work_id = ?1 AND kind = 'logo'
             ORDER BY priority, provider LIMIT 1",
            [work],
            |r| r.get(0),
        )
        .optional()?;
    let mut stmt = conn.prepare_cached(
        "SELECT provider, value, votes FROM work_ratings WHERE work_id = ?1 ORDER BY provider",
    )?;
    out.ratings = stmt
        .query_map([work], |r| {
            Ok(Rating {
                provider: r.get(0)?,
                value: r.get(1)?,
                votes: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut stmt = conn.prepare_cached(
        "SELECT provider, ext_kind, ext_id FROM external_ids WHERE work_id = ?1
         ORDER BY provider, ext_kind",
    )?;
    out.external_ids = stmt
        .query_map([work], |r| {
            Ok(ExternalId {
                provider: r.get(0)?,
                kind: r.get(1)?,
                id: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut stmt = conn.prepare_cached(
        "SELECT role, name, character, job, provider, person_ref, profile_url
         FROM work_credits WHERE work_id = ?1
         ORDER BY role = 'crew', provider, position",
    )?;
    out.credits = stmt
        .query_map([work], |r| {
            let provider: String = r.get(4)?;
            let person: Option<String> = r.get(5)?;
            Ok(Credit {
                crew: r.get::<_, String>(0)? == "crew",
                name: r.get(1)?,
                character: r.get(2)?,
                job: r.get(3)?,
                person_ref: person.map(|p| format!("{provider}:{p}")),
                profile_url: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(())
}

impl Catalog {
    /// Details of a movie or show item for the Detail screen (UI-009);
    /// episodes and tracks get the technical data of their file only.
    pub fn details(&self, item: ItemRef) -> Result<ItemDetails, Error> {
        let conn = self.lock();
        let mut out = ItemDetails::default();
        match item {
            ItemRef::Movie(w) | ItemRef::Show(w) => work_details(&conn, w, &mut out)?,
            ItemRef::Episode(_) | ItemRef::Track(_) => {}
        }
        if matches!(item, ItemRef::Movie(_) | ItemRef::Episode(_)) {
            let versions = lists::versions(&conn, item)?;
            if let Some(v) = versions.iter().find(|v| v.is_default) {
                out.tech = tech(&conn, v.file_id)?;
                if let ItemRef::Movie(_) = item {
                    let (extras, logo) = group_assets(&conn, v.file_id)?;
                    out.extras = extras;
                    out.local_logo = logo;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems() {
        assert_eq!(stem("Movies/X/Trailers/Main Trailer.mkv"), "Main Trailer");
        assert_eq!(stem("noext"), "noext");
        assert_eq!(stem(".hidden"), ".hidden");
    }
}
