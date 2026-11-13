// SPDX-License-Identifier: MPL-2.0
//! Anime cross-ID and episode-numbering mappings (META-008).
//!
//! Three upstream sources, with different licences (see
//! `docs/legal/anime-mappings.md`):
//!
//! | Source | Licence | How it reaches the device |
//! |---|---|---|
//! | anime-offline-database (cedya77 fork of manami-project) | ODbL-1.0 + DbCL-1.0 | bundled as a derived ODbL database ([`offline_db`]) and optionally refreshed from upstream |
//! | Anime-Lists `anime-list-master.xml` | none stated | **downloaded by the user's device from upstream only** ([`update`]) |
//! | Fribb `anime-list-full.json` | none stated | **downloaded by the user's device from upstream only** ([`update`]) |
//!
//! The crate never merges unlicensed data into a file that we distribute:
//! [`MappingIndex`] combines the layers in memory on the user's device.
//!
//! The crate is sans-IO for the network: [`update::Updater`] takes an
//! [`update::HttpClient`] implemented by the caller, so tests run offline.

pub mod anime_lists;
pub mod fribb;
pub mod ids;
pub mod index;
pub mod offline_db;
pub mod update;

pub use ids::{AnimeIds, IdRef};
pub use index::{EpisodeRef, MappingIndex, Record};

use std::fmt;

/// Error while parsing or validating mapping data.
#[derive(Debug)]
pub enum Error {
    /// Input is not valid for the expected format.
    Parse(String),
    /// Filesystem error (cache, bundled file).
    Io(std::io::Error),
    /// Data parsed but failed a sanity or integrity check.
    Invalid(String),
    /// The HTTP client reported an error.
    Http(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(m) => write!(f, "parse error: {m}"),
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Invalid(m) => write!(f, "invalid data: {m}"),
            Error::Http(m) => write!(f, "http error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Attribution lines for the "About" screen and `THIRD_PARTY_NOTICES`.
///
/// The first entry is mandatory whenever the bundled database is shipped
/// (ODbL §4.2/§4.3). The other two are shown when the user has downloaded the
/// corresponding list.
pub const ATTRIBUTION: &[(&str, &str)] = &[
    (
        "Contains information from anime-offline-database (cedya77, continuation of manami-project), made available under the ODbL v1.0 (https://opendatacommons.org/licenses/odbl/1-0/).",
        "https://github.com/cedya77/anime-offline-database https://github.com/manami-project/anime-offline-database",
    ),
    (
        "Episode mappings from Anime-Lists (anime-list-master.xml), downloaded by this device from upstream. Please send corrections upstream.",
        "https://github.com/Anime-Lists/anime-lists",
    ),
    (
        "Cross-ID mappings from Fribb/anime-lists (anime-list-full.json), downloaded by this device from upstream.",
        "https://github.com/Fribb/anime-lists",
    ),
];
