// SPDX-License-Identifier: MPL-2.0
//! Types of the library queries (LIB-009) for Dart: filters, sorts, pages,
//! the alphabet index, facets, folder entries, view preferences and smart
//! shelves. The calls are methods of
//! [`CoreLibrary`](crate::api::library::CoreLibrary).

use pa_catalog as c;
use pa_catalog::{HdrFilter, Resolution, SortField, WatchState};

use crate::api::library::{CoreItem, CoreItemKind};

/// Watch state of a movie or show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWatchState {
    Unwatched,
    InProgress,
    Watched,
}

/// Resolution class of the main video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreResolution {
    Sd,
    Hd,
    FullHd,
    Uhd,
}

/// Dynamic range filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreHdrFilter {
    Sdr,
    AnyHdr,
    Hdr10,
    Hdr10Plus,
    DolbyVision,
    Hlg,
}

/// Sort field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreSortField {
    Title,
    Year,
    Added,
    Duration,
    LastWatched,
}

/// `From` both ways between a Dart enum and the catalog enum of the same
/// variants.
macro_rules! same_enum {
    ($core:ident, $cat:ident, [$($v:ident),*]) => {
        impl From<$core> for $cat {
            fn from(v: $core) -> Self {
                match v { $($core::$v => Self::$v),* }
            }
        }
        impl From<$cat> for $core {
            fn from(v: $cat) -> Self {
                match v { $($cat::$v => Self::$v),* }
            }
        }
    };
}

same_enum!(CoreWatchState, WatchState, [Unwatched, InProgress, Watched]);
same_enum!(CoreResolution, Resolution, [Sd, Hd, FullHd, Uhd]);
same_enum!(
    CoreHdrFilter,
    HdrFilter,
    [Sdr, AnyHdr, Hdr10, Hdr10Plus, DolbyVision, Hlg]
);
same_enum!(
    CoreSortField,
    SortField,
    [Title, Year, Added, Duration, LastWatched]
);

fn conv<A: Copy, B: From<A>>(v: &[A]) -> Vec<B> {
    v.iter().map(|x| B::from(*x)).collect()
}

/// Filters: AND between fields, OR inside one; empty fields do not filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreLibraryFilter {
    /// Genre names, case-insensitive.
    pub genres: Vec<String>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub watched: Vec<CoreWatchState>,
    pub resolutions: Vec<CoreResolution>,
    pub hdr: Vec<CoreHdrFilter>,
    /// Audio format slugs: `aac`, `ac3`, `eac3`, `truehd`, `dts`,
    /// `dts_hd_ma`, `flac`, `atmos`, `dts_x`…
    pub audio_formats: Vec<String>,
    /// Audio languages as the probe stores them (`rus`, `eng`).
    pub audio_languages: Vec<String>,
    pub sources: Vec<i64>,
}

impl From<CoreLibraryFilter> for c::LibraryFilter {
    fn from(f: CoreLibraryFilter) -> Self {
        Self {
            genres: f.genres,
            year_min: f.year_min.map(i64::from),
            year_max: f.year_max.map(i64::from),
            watched: conv(&f.watched),
            resolutions: conv(&f.resolutions),
            hdr: conv(&f.hdr),
            audio_formats: f.audio_formats,
            audio_languages: f.audio_languages,
            sources: f.sources,
        }
    }
}

fn small(v: Option<i64>) -> Option<i32> {
    v.and_then(|v| i32::try_from(v).ok())
}

impl From<c::LibraryFilter> for CoreLibraryFilter {
    fn from(f: c::LibraryFilter) -> Self {
        Self {
            genres: f.genres,
            year_min: small(f.year_min),
            year_max: small(f.year_max),
            watched: conv(&f.watched),
            resolutions: conv(&f.resolutions),
            hdr: conv(&f.hdr),
            audio_formats: f.audio_formats,
            audio_languages: f.audio_languages,
            sources: f.sources,
        }
    }
}

/// Sort: missing values last in both directions, ties by title A→Z.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreLibrarySort {
    pub field: CoreSortField,
    pub descending: bool,
}

impl From<CoreLibrarySort> for c::LibrarySort {
    fn from(s: CoreLibrarySort) -> Self {
        Self {
            field: s.field.into(),
            descending: s.descending,
        }
    }
}

impl From<c::LibrarySort> for CoreLibrarySort {
    fn from(s: c::LibrarySort) -> Self {
        Self {
            field: s.field.into(),
            descending: s.descending,
        }
    }
}

/// Movies or shows (`Series`), filtered and sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreLibraryQuery {
    pub kind: CoreItemKind,
    pub filter: CoreLibraryFilter,
    pub sort: CoreLibrarySort,
}

impl From<CoreLibraryQuery> for c::LibraryQuery {
    fn from(q: CoreLibraryQuery) -> Self {
        Self {
            kind: q.kind.into(),
            filter: q.filter.into(),
            sort: q.sort.into(),
        }
    }
}

impl From<c::LibraryQuery> for CoreLibraryQuery {
    fn from(q: c::LibraryQuery) -> Self {
        Self {
            kind: q.kind.into(),
            filter: q.filter.into(),
            sort: q.sort.into(),
        }
    }
}

/// One page of a query.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreItemPage {
    pub items: Vec<CoreItem>,
    /// Matching items in total.
    pub total: u32,
    pub offset: u32,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<c::ItemPage> for CoreItemPage {
    fn from(p: c::ItemPage) -> Self {
        Self {
            items: p.items.into_iter().map(Into::into).collect(),
            total: count(p.total),
            offset: count(p.offset),
        }
    }
}

/// A letter of the alphabet index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAlphabetEntry {
    /// Upper-case letter, `#` for digits and symbols.
    pub letter: String,
    /// Position of the first item with this letter.
    pub offset: u32,
    pub count: u32,
}

impl From<c::AlphabetEntry> for CoreAlphabetEntry {
    fn from(e: c::AlphabetEntry) -> Self {
        Self {
            letter: e.letter,
            offset: count(e.offset),
            count: count(e.count),
        }
    }
}

/// A text filter value (genre, audio format, language) with its item count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTextFacet {
    pub value: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreResolutionFacet {
    pub value: CoreResolution,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreHdrFacet {
    pub value: CoreHdrFilter,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSourceFacet {
    pub source_id: i64,
    pub count: u32,
}

/// Filter values present among movies or shows (unfiltered counts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFacets {
    pub genres: Vec<CoreTextFacet>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
    pub resolutions: Vec<CoreResolutionFacet>,
    pub hdr: Vec<CoreHdrFacet>,
    pub audio_formats: Vec<CoreTextFacet>,
    pub audio_languages: Vec<CoreTextFacet>,
    pub sources: Vec<CoreSourceFacet>,
}

fn text(v: Vec<c::FacetValue<String>>) -> Vec<CoreTextFacet> {
    v.into_iter()
        .map(|f| CoreTextFacet {
            value: f.value,
            count: count(f.count),
        })
        .collect()
}

impl From<c::Facets> for CoreFacets {
    fn from(f: c::Facets) -> Self {
        Self {
            genres: text(f.genres),
            year_min: small(f.year_min),
            year_max: small(f.year_max),
            resolutions: f
                .resolutions
                .into_iter()
                .map(|r| CoreResolutionFacet {
                    value: r.value.into(),
                    count: count(r.count),
                })
                .collect(),
            hdr: f
                .hdr
                .into_iter()
                .map(|h| CoreHdrFacet {
                    value: h.value.into(),
                    count: count(h.count),
                })
                .collect(),
            audio_formats: text(f.audio_formats),
            audio_languages: text(f.audio_languages),
            sources: f
                .sources
                .into_iter()
                .map(|s| CoreSourceFacet {
                    source_id: s.value,
                    count: count(s.count),
                })
                .collect(),
        }
    }
}

/// An entry of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFolderEntry {
    pub name: String,
    /// Relative to the source root.
    pub rel_path: String,
    pub is_folder: bool,
    /// Files: id of the movie, episode or track, if recognized.
    pub item_id: Option<String>,
    pub size_bytes: i64,
    /// Folders: media files inside at any depth; files: 1.
    pub file_count: u32,
    pub added_at_ms: i64,
    pub duration_ms: Option<i64>,
}

impl From<c::FolderEntry> for CoreFolderEntry {
    fn from(e: c::FolderEntry) -> Self {
        Self {
            name: e.name,
            rel_path: e.rel_path,
            is_folder: e.is_folder,
            item_id: e.item.map(|i| i.to_string()),
            size_bytes: e.size_bytes,
            file_count: count(e.file_count),
            added_at_ms: e.added_at_ms,
            duration_ms: e.duration_ms,
        }
    }
}

/// Saved presentation of a view (`movies`, `shows`, `folder:<source>`…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreViewPrefs {
    pub view: String,
    pub sort: CoreLibrarySort,
    pub folders_first: bool,
    /// Last filter of the view, if kept.
    pub filter: Option<CoreLibraryFilter>,
}

impl From<CoreViewPrefs> for c::ViewPrefs {
    fn from(p: CoreViewPrefs) -> Self {
        Self {
            view: p.view,
            sort: p.sort.into(),
            folders_first: p.folders_first,
            filter: p.filter.map(Into::into),
        }
    }
}

impl From<c::ViewPrefs> for CoreViewPrefs {
    fn from(p: c::ViewPrefs) -> Self {
        Self {
            view: p.view,
            sort: p.sort.into(),
            folders_first: p.folders_first,
            filter: p.filter.map(Into::into),
        }
    }
}

/// A smart shelf: a saved query, optionally pinned to Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSmartShelf {
    pub id: i64,
    pub name: String,
    pub query: CoreLibraryQuery,
    pub pinned_home: bool,
    pub position: i64,
}

impl From<c::SmartShelf> for CoreSmartShelf {
    fn from(s: c::SmartShelf) -> Self {
        Self {
            id: s.id,
            name: s.name,
            query: s.query.into(),
            pinned_home: s.pinned_home,
            position: s.position,
        }
    }
}
