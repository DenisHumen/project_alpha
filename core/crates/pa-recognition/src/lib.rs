// SPDX-License-Identifier: MPL-2.0
//! Media recognition core of project_alpha (sans-IO).
//!
//! The crate follows the pipeline from `docs/research/03-metadata-recognition.md`
//! §4.2 and §7.2. It contains stage \[3\], the filename/path parser
//! ([`parse`]), which turns a relative media path into N-best
//! [`ParsedName`](parse::ParsedName) hypotheses with weights; stage \[4\],
//! title normalization and the local title index ([`title`]), which turns
//! those hypotheses into ranked work candidates without network requests;
//! stage \[5\], the scorer v0 ([`score`]), which picks the work, the
//! franchise entry and the episode for every file of a group;
//! and the [`provider`] boundary through which later stages ask metadata
//! providers for data without doing I/O themselves.
//!
//! ```
//! use pa_recognition::parse::parse;
//!
//! let result = parse("Anime/One Piece/[SubsPlease] One Piece - 1085 (1080p) [ABCD1234].mkv");
//! let best = &result.best().parsed;
//! assert_eq!(best.title.as_deref(), Some("One Piece"));
//! assert_eq!(best.release_group.as_deref(), Some("SubsPlease"));
//! assert_eq!(best.crc32.as_deref(), Some("ABCD1234"));
//! ```
//!
//! The crate does no I/O and contains no network code: everything is a pure
//! function of its input, so results are deterministic and reproducible in
//! the evaluation harness.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod parse;
pub mod provider;
pub mod score;
pub mod title;
