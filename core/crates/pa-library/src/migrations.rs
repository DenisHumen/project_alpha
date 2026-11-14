// SPDX-License-Identifier: MPL-2.0
//! Embedded migrations from `migrations/`.
//!
//! Files are named `YYYYMMDDHHMM_<ID>_<slug>.sql` (core/AGENTS.md). The
//! timestamp is the migration version, so branches merged in any order get a
//! total order without renumbering. The runner lives in `lib.rs` (ADR-0016,
//! amended by LIB-016). A test checks that every file in the directory is
//! listed here.

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
pub const MIGRATIONS: &[MigrationFile] = &[
    migration!(202611131200, "202611131200_LIB-001_schema_v1"),
    migration!(202611141200, "202611141200_SYNC-001_sync_outbox"),
    migration!(202611141400, "202611141400_LIB-011_progress_hidden"),
    migration!(202611141600, "202611141600_MUS-001_music_library"),
    migration!(202611141730, "202611141730_TWI-007_twitch"),
    migration!(202611141800, "202611141800_LIB-008_search_index"),
    migration!(202611142000, "202611142000_CORE-011_jobs_runtime"),
    migration!(202611142200, "202611142200_META-013_artwork_metadata"),
    migration!(202611142300, "202611142300_NET-011_source_manager"),
    migration!(202611150100, "202611150100_LIB-007_folder_structure"),
    migration!(202611151400, "202611151400_LIB-009_library_queries"),
    migration!(202611151900, "202611151900_LIB-010_versions_lists"),
    migration!(202611161000, "202611161000_UI-009_credits_ratings"),
];

impl MigrationFile {
    /// Checksum stored in the history table: FNV-1a 64 of the SQL text, as
    /// 16 hex digits. Stable across Rust versions and platforms.
    pub fn checksum(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.sql.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn checksum_is_stable_and_content_based() {
        let file = |version, sql| MigrationFile {
            name: "m",
            version,
            sql,
        };
        assert_eq!(
            file(1, "SELECT 1;").checksum(),
            file(2, "SELECT 1;").checksum()
        );
        assert_ne!(
            file(1, "SELECT 1;").checksum(),
            file(1, "SELECT 2;").checksum()
        );
        // FNV-1a 64 test vector: empty input is the offset basis.
        assert_eq!(file(3, "").checksum(), "cbf29ce484222325");
    }
}
