// SPDX-License-Identifier: MPL-2.0
//! Manual match fix ("Fix match" in the app).

use pa_library::rusqlite::{OptionalExtension, params};

use crate::items::ItemRef;
use crate::{Catalog, Error};

impl Catalog {
    /// Renames the work of an item (an episode: its show) and/or sets its
    /// year or TMDB id. Its matches become `user_set`, so later scans keep
    /// them; the old title stays as a synonym, so files named the old way
    /// still join this work. A track gets its title and year.
    pub fn fix_match(
        &self,
        item: ItemRef,
        title: Option<&str>,
        year: Option<i64>,
        tmdb_id: Option<i64>,
    ) -> Result<(), Error> {
        let title = title.map(str::trim).filter(|t| !t.is_empty());
        let year = year.filter(|y| (1800..=3000).contains(y));
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let now = crate::now_ms();
        let work = match item {
            ItemRef::Movie(w) | ItemRef::Show(w) => w,
            ItemRef::Episode(e) => tx
                .query_row("SELECT work_id FROM episodes WHERE id = ?1", [e], |r| r.get(0))
                .optional()?
                .ok_or_else(|| Error::NotFound(item.to_string()))?,
            ItemRef::Track(t) => {
                let n = tx.execute(
                    "UPDATE music_tracks SET title = ifnull(?2, title), year = ifnull(?3, year)
                     WHERE id = ?1",
                    params![t, title, year],
                )?;
                if n == 0 {
                    return Err(Error::NotFound(item.to_string()));
                }
                tx.commit()?;
                return Ok(());
            }
        };
        let kind: String = tx
            .query_row("SELECT kind FROM works WHERE id = ?1", [work], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::NotFound(item.to_string()))?;
        if let Some(t) = title {
            tx.execute(
                "UPDATE titles SET kind = 'synonym' WHERE work_id = ?1 AND kind = 'main'",
                [work],
            )?;
            tx.execute(
                "INSERT INTO titles (work_id, text, kind, source) VALUES (?1, ?2, 'main', '')
                 ON CONFLICT (work_id, kind, lang, script, text) DO NOTHING",
                params![work, t],
            )?;
            tx.execute(
                "UPDATE works SET title = ?2, updated_at_ms = ?3 WHERE id = ?1",
                params![work, t, now],
            )?;
        }
        if title.is_some() || year.is_some() {
            tx.execute(
                "UPDATE works SET year = ?2, updated_at_ms = ?3 WHERE id = ?1",
                params![work, year, now],
            )?;
        }
        if let Some(id) = tmdb_id.filter(|i| *i > 0) {
            let ext_kind = if kind == "movie" { "movie" } else { "tv" };
            tx.execute(
                "DELETE FROM external_ids
                 WHERE provider = 'tmdb' AND ext_kind = ?2 AND (work_id = ?1 OR ext_id = ?3)",
                params![work, ext_kind, id.to_string()],
            )?;
            tx.execute(
                "INSERT INTO external_ids (work_id, provider, ext_kind, ext_id) VALUES (?1, 'tmdb', ?2, ?3)",
                params![work, ext_kind, id.to_string()],
            )?;
        }
        tx.execute(
            "UPDATE matches SET state = 'user_set', decided_by = 'user', confidence = 1.0,
                                updated_at_ms = ?2
             WHERE work_id = ?1 AND state <> 'rejected'",
            params![work, now],
        )?;
        tx.commit()?;
        Ok(())
    }
}
