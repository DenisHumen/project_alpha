// SPDX-License-Identifier: MPL-2.0
//! The files to play for an item (LIB-020): a movie split over several
//! files — the VOBs of a DVD title set, `cd1`/`cd2` parts — plays them back
//! to back as one timeline.
//!
//! The folder analysis (LIB-007, LIB-017) records such files as members of
//! one group with the role `part` and a part number. An item and its
//! versions point at the first part ([`Item::rel_path`](crate::Item::rel_path),
//! [`Version::rel_path`](crate::Version::rel_path)); [`Catalog::play_parts`]
//! returns all of them in order. How the player joins them is up to the
//! client: the app hands libmpv one `edl://` URI (ADR-0043, LIB-020).

use pa_library::rusqlite::{Connection, OptionalExtension, params};

use crate::items::{local_path, relative};
use crate::{Catalog, Error};

/// One file of what an item plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayPart {
    /// Path relative to the source root, like
    /// [`Item::rel_path`](crate::Item::rel_path).
    pub rel_path: String,
    /// Absolute path for local sources, `None` for network ones (the app
    /// plays them through the stream proxy).
    pub local_path: Option<String>,
    pub size_bytes: i64,
    /// From `pa-probe`, if the file was probed.
    pub duration_ms: Option<i64>,
}

/// The present files of the split-file or disc group that `file` is a part
/// of, by part number.
const PARTS_SQL: &str = "
SELECT f.path, f.size_bytes, f.duration_ms, s.uri
FROM file_group_members gm
JOIN files f ON f.id = gm.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
WHERE gm.role = 'part'
  AND gm.group_id = (SELECT pm.group_id FROM file_group_members pm
                      WHERE pm.file_id = ?1 AND pm.role = 'part'
                      ORDER BY pm.group_id LIMIT 1)
ORDER BY gm.part_no, f.path";

const FILE_SQL: &str = "
SELECT f.id, f.path, f.size_bytes, f.duration_ms, s.uri
FROM files f
JOIN sources s ON s.id = f.source_id
WHERE f.source_id = ?1 AND f.path = ?2 AND f.missing_since_ms IS NULL";

fn part(path: &str, size: i64, duration: Option<i64>, uri: &str) -> PlayPart {
    let rel = relative(path);
    PlayPart {
        local_path: local_path(uri, &rel),
        rel_path: rel,
        size_bytes: size,
        duration_ms: duration,
    }
}

pub(crate) fn play_parts(
    conn: &Connection,
    source_id: i64,
    rel_path: &str,
) -> Result<Vec<PlayPart>, Error> {
    let path = format!("/{}", rel_path.trim_start_matches('/'));
    let file = conn
        .prepare_cached(FILE_SQL)?
        .query_row(params![source_id, path], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                part(
                    &r.get::<_, String>(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    &r.get::<_, String>(4)?,
                ),
            ))
        })
        .optional()?;
    let Some((file_id, single)) = file else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare_cached(PARTS_SQL)?;
    let parts = stmt
        .query_map([file_id], |r| {
            Ok(part(
                &r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                &r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if parts.is_empty() { vec![single] } else { parts })
}

impl Catalog {
    /// The files that play the file `rel_path` of source `source_id` (an
    /// [`Item::rel_path`](crate::Item::rel_path) or a
    /// [`Version::rel_path`](crate::Version::rel_path)) as one movie: every
    /// part of its split file or DVD title set in order, from part 1 even
    /// if `rel_path` is a later part; the file alone otherwise. Missing
    /// parts are left out. Empty for an unknown or missing file.
    pub fn play_parts(&self, source_id: i64, rel_path: &str) -> Result<Vec<PlayPart>, Error> {
        play_parts(&self.lock(), source_id, rel_path)
    }
}
