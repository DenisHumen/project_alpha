// SPDX-License-Identifier: MPL-2.0
//! Shallow incremental scan of a media source (LIB-003).
//!
//! The first stage of the library pipeline (`docs/dev/architecture.md` §6,
//! `docs/research/04` §1.4): list a source through [`pa_vfs::Vfs`] and keep
//! `files` in [`pa_library`] in step with it, using only the listing
//! (path, size, mtime). No file is opened: probing (LIB-006), hashing
//! (LIB-004) and folder analysis ([`analyze_folders`], LIB-007) run later.
//!
//! - [`scan`] walks the source in parallel, inserts new files, updates
//!   changed ones and soft-deletes the ones that disappeared; rows missing
//!   longer than the grace period are deleted.
//! - A rescan without changes writes nothing.
//! - Progress comes as [`ScanEvent`]s; [`CancelToken`] stops a scan.
//! - [`ScanRules`] decides what is recorded; the defaults are the
//!   release-0.1 rules (samples, extras, BDMV/VIDEO_TS, NAS service folders
//!   and hidden entries are skipped).
//! - [`index_music`] (MUS-001) runs after [`scan`]: it reads the tags of new
//!   and changed audio files with `lofty` ([`read_tags`], path fallback
//!   [`path_info`]) and fills the music tables of `pa_library::music`
//!   (artists, albums, tracks, genres, ReplayGain, covers).
//! - [`analyze_folders`] (LIB-007) runs after [`scan`]: it lists the source
//!   with the folders and sidecars the scan skips and stores the structure
//!   of every folder ([`structure::analyze`]): discs (BDMV, VIDEO_TS, ISO)
//!   as one item, joined parts (`cd1`/`part2`/`disc1`), extras and samples,
//!   sidecar subtitles, NFO ids and local art, in `pa_library::groups`.
//!
//! ```
//! use pa_scanner::{CancelToken, ScanOptions, scan};
//! use pa_vfs::LocalFs;
//!
//! let dir = tempfile::tempdir()?;
//! std::fs::create_dir(dir.path().join("Dune (2021)"))?;
//! std::fs::write(dir.path().join("Dune (2021)/Dune (2021).mkv"), b"video")?;
//! std::fs::write(dir.path().join("Dune (2021)/Dune-trailer.mkv"), b"extra")?;
//!
//! let mut conn = pa_library::open_in_memory()?;
//! conn.execute(
//!     "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'Movies', 'file:///movies', 0)",
//!     [],
//! )?;
//! let vfs = LocalFs::new(dir.path())?;
//! let report = scan(&mut conn, 1, &vfs, &ScanOptions::default(), &CancelToken::new(), &mut |_| {})?;
//! assert_eq!(report.added, 1); // the trailer is skipped
//!
//! let again = scan(&mut conn, 1, &vfs, &ScanOptions::default(), &CancelToken::new(), &mut |_| {})?;
//! assert_eq!((again.added, again.unchanged), (0, 1));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod folders;
mod music;
mod rules;
mod scan;
pub mod structure;
mod tags;
mod walk;

pub use folders::{FolderOptions, FolderReport, analyze_folders};

pub use music::{COVER_EXTENSIONS, COVER_NAMES, MusicOptions, MusicReport, index_music};

pub use rules::{
    AUDIO_EXTENSIONS, IGNORED_FOLDERS, MediaKind, ScanRules, VIDEO_EXTENSIONS, is_extra_file,
    is_ignored_folder, media_kind,
};
pub use scan::{
    CancelToken, DEFAULT_GRACE_PERIOD, ScanError, ScanEvent, ScanOptions, ScanProgress, ScanReport,
    scan,
};
pub use tags::{
    EmbeddedArt, PathInfo, TagError, path_info, read_embedded_art, read_tags, with_path_fallback,
};
