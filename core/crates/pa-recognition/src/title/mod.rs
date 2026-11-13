// SPDX-License-Identifier: MPL-2.0
//! Title normalization and the local title index (META-007): candidate
//! generation without network requests, stage \[4\] of research 03 §4.2.
//!
//! - [`normalize`] — match keys: Unicode and punctuation folding, roman
//!   numerals, articles, Cyrillic transliteration and romaji variants in one
//!   loose Latin space, season/part/year qualifiers.
//! - [`translit`] — Russian Cyrillic ↔ Latin (GOST 7.79-2000 B and the
//!   everyday scheme) for display and provider queries.
//! - [`index`] — [`TitleIndex`]: exact, token (IDF) and trigram lookups over
//!   titles and aliases of [`TitleRecord`]s, ranked [`Candidate`]s for a
//!   [`TitleQuery`] built from the parser's N-best hypotheses.
//! - [`anime`] — records from the bundled anime-offline-database.
//!
//! ```
//! use pa_recognition::parse::parse;
//! use pa_recognition::title::{TitleIndex, TitleQuery, TitleRecord, WorkKind};
//!
//! let index = TitleIndex::from_records([TitleRecord {
//!     kind: WorkKind::Series,
//!     anime: true,
//!     year: Some(2022),
//!     titles: vec!["Mob Psycho 100 III".into(), "Моб Психо 100 III".into()],
//!     ..Default::default()
//! }]);
//! let query = TitleQuery::from_parse(&parse("Аниме/Моб Психо 100 3 сезон/Mob Psycho 100 - 05.mkv"));
//! assert_eq!(index.search(&query, 10)[0].record, 0);
//! ```

pub mod anime;
pub mod index;
pub mod normalize;
pub mod translit;

pub use index::{Candidate, MatchTier, TitleIndex, TitleQuery, TitleRecord, WorkKind};
pub use normalize::TitleKey;
