// SPDX-License-Identifier: MPL-2.0
//! Media sources (`sources`).

use std::path::PathBuf;

use pa_library::rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Catalog, Error, uri};

/// A place the library scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// `sources.id`.
    pub id: i64,
    /// `local` (a folder) or `smb`.
    pub kind: String,
    pub name: String,
    /// Root URI without credentials: `file:///…` or `smb://host/share/dir`.
    pub uri: String,
    pub username: Option<String>,
    /// Local path of the root for `file://` sources.
    pub local_root: Option<PathBuf>,
}

const COLUMNS: &str = "id, kind, name, uri, username";

fn source_from_row(r: &Row<'_>) -> pa_library::rusqlite::Result<Source> {
    let uri: String = r.get(3)?;
    Ok(Source {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        local_root: uri::file_uri_to_path(&uri),
        uri,
        username: r.get(4)?,
    })
}

pub(crate) fn list(conn: &Connection) -> Result<Vec<Source>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM sources ORDER BY position, name, id"
    ))?;
    let rows = stmt.query_map([], source_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub(crate) fn get(conn: &Connection, id: i64) -> Result<Option<Source>, Error> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM sources WHERE id = ?1"),
            [id],
            source_from_row,
        )
        .optional()?)
}

/// Id of the source with this URI, if any.
pub(crate) fn by_uri(conn: &Connection, uri: &str) -> Result<Option<i64>, Error> {
    Ok(conn
        .query_row("SELECT id FROM sources WHERE uri = ?1", [uri], |r| r.get(0))
        .optional()?)
}

/// Inserts a source and returns its id.
pub(crate) fn insert(
    conn: &Connection,
    kind: &str,
    name: &str,
    uri: &str,
    username: Option<&str>,
    now: i64,
) -> Result<i64, Error> {
    conn.execute(
        "INSERT INTO sources (kind, name, uri, username, position, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, (SELECT ifnull(max(position), 0) + 1 FROM sources), ?5)",
        params![kind, name, uri, username, now],
    )?;
    Ok(conn.last_insert_rowid())
}

impl Catalog {
    /// Sources in display order.
    pub fn sources(&self) -> Result<Vec<Source>, Error> {
        list(&self.lock())
    }

    /// Adds a source, or updates source `id` if given. `kind` is `local`
    /// (then `location` is an absolute folder path) or `smb` (then it is an
    /// `smb://` URI).
    pub fn add_source(
        &self,
        id: Option<i64>,
        kind: &str,
        name: &str,
        location: &str,
        username: Option<&str>,
    ) -> Result<Source, Error> {
        let uri = match kind {
            "local" => {
                let path = PathBuf::from(location);
                if !path.is_absolute() {
                    return Err(Error::Invalid(format!("not an absolute path: {location}")));
                }
                uri::path_to_file_uri(&path)
            }
            "smb" => {
                if !location.starts_with("smb://") {
                    return Err(Error::Invalid(format!("not an smb:// URI: {location}")));
                }
                location.to_owned()
            }
            other => return Err(Error::Invalid(format!("source kind {other}"))),
        };
        let conn = self.lock();
        let id = match id {
            Some(id) if get(&conn, id)?.is_some() => {
                conn.execute(
                    "UPDATE sources SET kind = ?2, name = ?3, uri = ?4, username = ?5 WHERE id = ?1",
                    params![id, kind, name, uri, username],
                )?;
                id
            }
            _ => insert(&conn, kind, name, &uri, username, crate::now_ms())?,
        };
        get(&conn, id)?.ok_or_else(|| Error::NotFound(format!("source {id}")))
    }

    /// Removes a source with its files, matches and tracks, and the works,
    /// episodes and music entities left without files.
    pub fn remove_source(&self, id: i64) -> Result<(), Error> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sources WHERE id = ?1", [id])?;
        crate::recognize::remove_orphans(&tx)?;
        pa_library::music::remove_orphans(&tx)?;
        tx.commit()?;
        pa_library::search::sync(&conn)?;
        Ok(())
    }
}
