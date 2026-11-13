// SPDX-License-Identifier: MPL-2.0
//! Embedded migrations from `migrations/`.
//!
//! Files are named `YYYYMMDDHHMM_<ID>_<slug>.sql` (core/AGENTS.md). The
//! timestamp is the refinery version, so branches merged in any order get a
//! total order without renumbering; refinery's own `V<n>__name` naming is
//! derived from the file name. A test checks that every file in the directory
//! is listed here.

/// One embedded migration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationFile {
    /// File name without `.sql`.
    pub name: &'static str,
    /// `YYYYMMDDHHMM` prefix of the name.
    pub version: i64,
    pub sql: &'static str,
}

macro_rules! migration {
    ($version:literal, $name:literal) => {
        MigrationFile {
            name: $name,
            version: $version,
            sql: include_str!(concat!("../migrations/", $name, ".sql")),
        }
    };
}

/// All migrations, oldest first. Append only; never edit an applied file.
pub const MIGRATIONS: &[MigrationFile] =
    &[migration!(202611131200, "202611131200_LIB-001_schema_v1")];

impl MigrationFile {
    /// Refinery name `V<version>__<slug>` (refinery allows only `\w` in it).
    pub fn refinery_name(&self) -> String {
        let slug: String = self
            .name
            .split_once('_')
            .map_or(self.name, |(_, rest)| rest)
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("V{}__{}", self.version, slug)
    }
}

pub(crate) fn runner_migrations() -> Result<Vec<refinery::Migration>, refinery::Error> {
    MIGRATIONS
        .iter()
        .map(|m| refinery::Migration::unapplied(&m.refinery_name(), m.sql))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinery_name_is_derived_from_file_name() {
        assert_eq!(
            MIGRATIONS[0].refinery_name(),
            "V202611131200__lib_001_schema_v1"
        );
    }

    #[test]
    fn versions_match_names_and_increase() {
        let mut prev = 0;
        for m in MIGRATIONS {
            assert!(m.name.starts_with(&format!("{}_", m.version)), "{}", m.name);
            assert_eq!(m.version.to_string().len(), 12, "{}", m.name);
            assert!(m.version > prev, "{} is out of order", m.name);
            prev = m.version;
        }
    }

    #[test]
    fn refinery_accepts_every_migration() {
        let ms = runner_migrations().unwrap();
        assert_eq!(ms.len(), MIGRATIONS.len());
        for (r, m) in ms.iter().zip(MIGRATIONS) {
            assert_eq!(r.version(), m.version);
        }
    }
}
