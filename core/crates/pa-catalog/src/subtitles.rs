// SPDX-License-Identifier: MPL-2.0
//! Sidecar subtitles of a video file (PLAY-007): the subtitle files next to
//! it that the folder analysis (LIB-007, `group_assets`) attached to it or
//! to its single-movie folder, with the language and flags read from their
//! names (`Movie.ru.forced.srt`).
//!
//! The player loads them on open: local files by path, files of network
//! sources through the loopback stream proxy (ADR-0038), like the video.

use pa_library::rusqlite::{Connection, params};

use crate::items::{local_path, relative};
use crate::{Catalog, Error};

/// A subtitle file that belongs to a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    /// Path relative to the source root, like [`Item::rel_path`](crate::Item::rel_path).
    pub rel_path: String,
    /// Absolute path for local sources, `None` for network ones (the app
    /// plays them through the stream proxy).
    pub local_path: Option<String>,
    /// ISO 639-2 code (`rus`, `eng`) if the name tells.
    pub lang: Option<String>,
    /// Forced subtitles: only the lines in a foreign language.
    pub forced: bool,
    /// SDH / hearing impaired / closed captions.
    pub hearing_impaired: bool,
    /// Marked as the default track in its name.
    pub default: bool,
}

/// Subtitles named after the file come first, then the subtitles of its
/// folder (`Subs/English.srt` in a single-movie folder or a disc). A
/// subtitle without a target in a folder of several videos (a season) is
/// left out: nothing says which episode it is for.
const SIDECARS_SQL: &str = "
SELECT ga.path, ga.lang, ga.forced, ga.hearing_impaired, ga.is_default, s.uri
FROM files f
JOIN sources s ON s.id = f.source_id
JOIN file_group_members gm ON gm.file_id = f.id
JOIN file_groups g ON g.id = gm.group_id
JOIN group_assets ga ON ga.group_id = g.id
WHERE f.source_id = ?1 AND f.path = ?2 AND ga.role = 'subtitle'
  AND (ga.target_path = f.path
       OR (ga.target_path IS NULL AND g.kind IN ('movie_folder', 'disc')))
GROUP BY ga.path
ORDER BY ga.target_path IS NULL, ga.path";

pub(crate) fn sidecars(
    conn: &Connection,
    source_id: i64,
    rel_path: &str,
) -> Result<Vec<Sidecar>, Error> {
    let path = format!("/{}", rel_path.trim_start_matches('/'));
    let mut stmt = conn.prepare_cached(SIDECARS_SQL)?;
    let rows = stmt.query_map(params![source_id, path], |r| {
        let stored: String = r.get(0)?;
        let uri: String = r.get(5)?;
        let rel = relative(&stored);
        Ok(Sidecar {
            local_path: local_path(&uri, &rel),
            rel_path: rel,
            lang: r.get(1)?,
            forced: r.get::<_, i64>(2)? != 0,
            hearing_impaired: r.get::<_, i64>(3)? != 0,
            default: r.get::<_, i64>(4)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

impl Catalog {
    /// Sidecar subtitles of the file `rel_path` (relative to the root of
    /// source `source_id`, as [`Item::rel_path`](crate::Item::rel_path) and
    /// [`Version::rel_path`](crate::Version::rel_path)). Empty for an
    /// unknown file or one without subtitles.
    pub fn sidecar_subtitles(&self, source_id: i64, rel_path: &str) -> Result<Vec<Sidecar>, Error> {
        sidecars(&self.lock(), source_id, rel_path)
    }
}
