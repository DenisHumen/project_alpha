// SPDX-License-Identifier: MPL-2.0
//! Container analysis over libavformat (LIB-006, `docs/research/03` §4.2 [2]).
//!
//! [`probe`] reads the head (8 MiB by default) and tail (4 MiB) of a file
//! through any [`pa_vfs::ByteSource`] — local, SMB, WebDAV — and reports
//! [`MediaInfo`]: container, duration and how it was obtained, video
//! (codec, profile, resolution, frame rate, bit depth, colour, HDR10,
//! HDR10+, HLG, Dolby Vision profile and compatibility id), audio (codec,
//! channels and layout, language, Atmos/DTS:X), subtitles (text or bitmap,
//! language, forced/SDH), chapters, tags and attachments.
//!
//! ```no_run
//! use std::sync::Arc;
//! use pa_vfs::{LocalFs, Vfs, VfsPath};
//!
//! let vfs = LocalFs::new("/media")?;
//! let file = vfs.open(&VfsPath::parse("/Movies/Dune (2021).mkv")?)?;
//! let info = pa_probe::probe(Arc::from(file), &pa_probe::ProbeOptions::default())?;
//! println!("{} {:?} ms", info.container, info.duration_ms);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! For the library pipeline, [`store::probe_pending`] probes the files a
//! scan left unprobed and writes `container`, `duration_ms`, `video_json`,
//! `audio_langs` and `sub_langs` of `pa-library`'s `files` table, where the
//! recognition scorer reads durations.
//!
//! # Licensing and loading
//!
//! FFmpeg is LGPL and is never linked: [`Ffmpeg`] opens libavutil,
//! libavcodec and libavformat at run time with `libloading`, from
//! `$PA_FFMPEG_LIB_DIR` or through the platform loader. Supported: FFmpeg
//! 7.x–9.x (libavformat 61–63), whose public struct prefixes this crate
//! mirrors (see `sys`); anything else is
//! [`ProbeErrorKind::LibraryUnavailable`], as is a missing library. The
//! `AVIOContext` comes from `pa-player-io` (ADR-0027).
//!
//! HDR10+ and Dolby Vision RPUs without a configuration record are found
//! by scanning the first HEVC/AV1 packets in pure Rust (no decoding).
//! Atmos and DTS:X come from FFmpeg's codec profile and are only as good as
//! FFmpeg's detection within the analysed packets.

mod bitstream;
mod error;
mod library;
mod model;
mod probe;
pub mod store;
mod sys;
mod window;

pub use error::{ProbeError, ProbeErrorKind};
pub use library::{Ffmpeg, LIB_DIR_ENV, file_name};
pub use model::{
    Attachment, AudioStream, Chapter, DolbyVision, DurationSource, HdrFormat, MediaInfo,
    ObjectAudio, StreamCommon, SubtitleKind, SubtitleStream, VideoStream,
};
pub use probe::{
    DEFAULT_HEAD_BYTES, DEFAULT_TAIL_BYTES, MIN_HEAD_BYTES, ProbeOptions, probe, probe_path,
};
