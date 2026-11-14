// SPDX-License-Identifier: MPL-2.0
//! Per-view preferences, smart shelves and work genres (LIB-009).
//!
//! - [`ViewPrefs`]: the sort of a view (`movies`, `shows`, `folder:<source>`…)
//!   kept separately for every view and profile, its direction, the "folders
//!   first" option and, optionally, the last filter of the view.
//! - [`SmartShelf`]: a saved query of a profile (filter and sort, as JSON
//!   owned by the caller — `pa-catalog`'s `LibraryQuery`), optionally pinned
//!   to Home.
//! - [`set_work_genres`] / [`genres`]: genres of works by provider, for the
//!   genre filter.
//!
//! The query JSON is opaque here: this crate stores it and checks only that
//! it is valid JSON.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Error;

/// Saved presentation of one view for one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPrefs {
    /// View key, 1–128 characters (`movies`, `shows`, `folder:3`…).
    pub view: String,
    /// Sort field slug (`[a-z0-9_]+`), e.g. `title`, `added`.
    pub sort: String,
    pub descending: bool,
    /// Folders before files in folder views.
    pub folders_first: bool,
    /// Last filter of the view as JSON, if the view keeps it.
    pub filter_json: Option<String>,
    pub updated_at_ms: i64,
}

impl ViewPrefs {
    /// Defaults of a view nobody configured: by title, ascending, folders
    /// first, no filter.
    pub fn default_for(view: &str) -> Self {
        Self {
            view: view.to_owned(),
            sort: "title".to_owned(),
            descending: false,
            folders_first: true,
            filter_json: None,
            updated_at_ms: 0,
        }
    }
}

fn check_view(view: &str) -> Result<(), Error> {
    if view.is_empty() || view.chars().count() > 128 {
        return Err(Error::Invalid("view key"));
    }
    Ok(())
}

fn check_slug(s: &str) -> Result<(), Error> {
    if s.is_empty()
        || !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(Error::Invalid("sort slug"));
    }
    Ok(())
}

fn check_json(s: &str) -> Result<(), Error> {
    serde_json::from_str::<serde_json::Value>(s).map_err(|_| Error::Invalid("query JSON"))?;
    Ok(())
}

fn prefs_row(r: &Row<'_>) -> rusqlite::Result<ViewPrefs> {
    Ok(ViewPrefs {
        view: r.get(0)?,
        sort: r.get(1)?,
        descending: r.get(2)?,
        folders_first: r.get(3)?,
        filter_json: r.get(4)?,
        updated_at_ms: r.get(5)?,
    })
}

/// Saved preferences of `view`, `None` if the profile never saved any.
pub fn view_prefs(conn: &Connection, profile_id: i64, view: &str) -> Result<Option<ViewPrefs>, Error> {
    Ok(conn
        .prepare_cached(
            "SELECT view, sort, descending, folders_first, filter_json, updated_at_ms
             FROM view_prefs WHERE profile_id = ?1 AND view = ?2",
        )?
        .query_row(params![profile_id, view], prefs_row)
        .optional()?)
}

/// All saved view preferences of a profile, by view key.
pub fn all_view_prefs(conn: &Connection, profile_id: i64) -> Result<Vec<ViewPrefs>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT view, sort, descending, folders_first, filter_json, updated_at_ms
         FROM view_prefs WHERE profile_id = ?1 ORDER BY view",
    )?;
    let rows = stmt.query_map([profile_id], prefs_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Saves (replaces) the preferences of `prefs.view`.
pub fn set_view_prefs(conn: &Connection, profile_id: i64, prefs: &ViewPrefs) -> Result<(), Error> {
    check_view(&prefs.view)?;
    check_slug(&prefs.sort)?;
    if let Some(f) = &prefs.filter_json {
        check_json(f)?;
    }
    conn.prepare_cached(
        "INSERT INTO view_prefs (profile_id, view, sort, descending, folders_first, filter_json,
                                 updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (profile_id, view) DO UPDATE SET
             sort = excluded.sort, descending = excluded.descending,
             folders_first = excluded.folders_first, filter_json = excluded.filter_json,
             updated_at_ms = excluded.updated_at_ms",
    )?
    .execute(params![
        profile_id,
        prefs.view,
        prefs.sort,
        prefs.descending,
        prefs.folders_first,
        prefs.filter_json,
        prefs.updated_at_ms
    ])?;
    Ok(())
}

/// Forgets the preferences of `view` (back to the defaults).
pub fn reset_view_prefs(conn: &Connection, profile_id: i64, view: &str) -> Result<(), Error> {
    conn.prepare_cached("DELETE FROM view_prefs WHERE profile_id = ?1 AND view = ?2")?
        .execute(params![profile_id, view])?;
    Ok(())
}

/// A saved query of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelf {
    pub id: i64,
    pub profile_id: i64,
    pub name: String,
    /// The query as JSON (owned by the caller).
    pub query_json: String,
    /// Shown on Home.
    pub pinned_home: bool,
    /// Order on Home and in the list of shelves (lower first).
    pub position: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

fn shelf_row(r: &Row<'_>) -> rusqlite::Result<SmartShelf> {
    Ok(SmartShelf {
        id: r.get(0)?,
        profile_id: r.get(1)?,
        name: r.get(2)?,
        query_json: r.get(3)?,
        pinned_home: r.get(4)?,
        position: r.get(5)?,
        created_at_ms: r.get(6)?,
        updated_at_ms: r.get(7)?,
    })
}

const SHELF_COLUMNS: &str =
    "id, profile_id, name, query_json, pinned_home, position, created_at_ms, updated_at_ms";

/// Shelves of a profile by position; with `pinned_only`, only those pinned
/// to Home.
pub fn shelves(conn: &Connection, profile_id: i64, pinned_only: bool) -> Result<Vec<SmartShelf>, Error> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {SHELF_COLUMNS} FROM smart_shelves
         WHERE profile_id = ?1 AND (?2 = 0 OR pinned_home = 1)
         ORDER BY position, id"
    ))?;
    let rows = stmt.query_map(params![profile_id, pinned_only], shelf_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// One shelf, `None` if it does not exist.
pub fn shelf(conn: &Connection, id: i64) -> Result<Option<SmartShelf>, Error> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {SHELF_COLUMNS} FROM smart_shelves WHERE id = ?1"
        ))?
        .query_row([id], shelf_row)
        .optional()?)
}

/// Creates a shelf at the end of the profile's list and returns it.
pub fn create_shelf(
    conn: &Connection,
    profile_id: i64,
    name: &str,
    query_json: &str,
    pinned_home: bool,
    now_ms: i64,
) -> Result<SmartShelf, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("shelf name"));
    }
    check_json(query_json)?;
    conn.prepare_cached(
        "INSERT INTO smart_shelves (profile_id, name, query_json, pinned_home, position,
                                    created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4,
                 (SELECT ifnull(max(position) + 1, 0) FROM smart_shelves WHERE profile_id = ?1),
                 ?5, ?5)",
    )?
    .execute(params![profile_id, name, query_json, pinned_home, now_ms])?;
    let id = conn.last_insert_rowid();
    shelf(conn, id)?.ok_or(Error::Invalid("shelf"))
}

/// Changes name, query and pin of a shelf. `false` if it does not exist.
pub fn update_shelf(
    conn: &Connection,
    id: i64,
    name: &str,
    query_json: &str,
    pinned_home: bool,
    now_ms: i64,
) -> Result<bool, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("shelf name"));
    }
    check_json(query_json)?;
    let n = conn
        .prepare_cached(
            "UPDATE smart_shelves SET name = ?2, query_json = ?3, pinned_home = ?4,
                 updated_at_ms = ?5
             WHERE id = ?1",
        )?
        .execute(params![id, name, query_json, pinned_home, now_ms])?;
    Ok(n > 0)
}

/// Deletes a shelf. `false` if it did not exist.
pub fn delete_shelf(conn: &Connection, id: i64) -> Result<bool, Error> {
    Ok(conn
        .prepare_cached("DELETE FROM smart_shelves WHERE id = ?1")?
        .execute([id])?
        > 0)
}

/// Puts the profile's shelves in the order of `ids` (positions 0, 1, …);
/// shelves not listed keep their relative order after them.
pub fn reorder_shelves(conn: &mut Connection, profile_id: i64, ids: &[i64], now_ms: i64) -> Result<(), Error> {
    let tx = conn.transaction()?;
    let current: Vec<i64> = {
        let mut stmt = tx.prepare_cached(
            "SELECT id FROM smart_shelves WHERE profile_id = ?1 ORDER BY position, id",
        )?;
        stmt.query_map([profile_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    let mut order: Vec<i64> = Vec::with_capacity(current.len());
    for id in ids {
        if current.contains(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    for id in current {
        if !order.contains(&id) {
            order.push(id);
        }
    }
    {
        let mut stmt = tx.prepare_cached(
            "UPDATE smart_shelves SET position = ?2, updated_at_ms = ?3
             WHERE id = ?1 AND position <> ?2",
        )?;
        for (pos, id) in order.iter().enumerate() {
            stmt.execute(params![id, i64::try_from(pos).unwrap_or(i64::MAX), now_ms])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Replaces the genres of `work_id` from `provider` with `names` (trimmed,
/// empty and duplicate names skipped).
pub fn set_work_genres(
    conn: &Connection,
    work_id: i64,
    provider: &str,
    names: &[&str],
) -> Result<(), Error> {
    conn.prepare_cached("DELETE FROM work_genres WHERE work_id = ?1 AND provider = ?2")?
        .execute(params![work_id, provider])?;
    let mut ins = conn.prepare_cached(
        "INSERT OR IGNORE INTO work_genres (work_id, provider, name) VALUES (?1, ?2, ?3)",
    )?;
    for name in names {
        let name = name.trim();
        if !name.is_empty() {
            ins.execute(params![work_id, provider, name])?;
        }
    }
    Ok(())
}

/// Genres of a work, by name (case-insensitive), without duplicates
/// between providers.
pub fn genres(conn: &Connection, work_id: i64) -> Result<Vec<String>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT min(name) FROM work_genres WHERE work_id = ?1
         GROUP BY name COLLATE NOCASE ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([work_id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}
