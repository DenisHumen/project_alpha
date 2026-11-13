// SPDX-License-Identifier: MPL-2.0
//! Player I/O adapters: FFmpeg and libmpv read media straight from a
//! [`pa_vfs::ByteSource`] (local file, SMB, WebDAV…) without temp files or
//! a loopback HTTP hop (NET-008, `docs/research/02` §4, ADR-0013).
//!
//! - [`ffmpeg`]: `AVIOContext` callbacks (`read_packet`, `seek`, interrupt)
//!   and an owning [`ffmpeg::AvioContext`] for libavformat custom I/O.
//! - [`mpv`]: a `stream_cb` protocol handler, so `loadfile pa://<id>` plays
//!   a source looked up by a [`mpv::SourceResolver`].
//!
//! Both adapters are thin: a cursor over the positional
//! [`read_at`](pa_vfs::ByteSource::read_at), seek by moving the cursor,
//! size from [`size`](pa_vfs::ByteSource::size). Read-ahead and prefetch
//! belong to the network backend's `ByteSource`, not here.
//!
//! # Licensing
//!
//! The crate does not link FFmpeg or libmpv. The library functions it calls
//! (`avio_alloc_context`, `mpv_stream_cb_add_ro`…) are passed in as
//! function pointers by the code that already loads our LGPL shared builds
//! (`tools/ffmpeg`, `tools/mpv`), so the LGPL libraries stay dynamically
//! linked and replaceable.
//!
//! # Errors and cancellation
//!
//! A failing source returns `AVERROR(EIO)` / `-1` to the player; the
//! original [`pa_vfs::VfsError`] (with its kind, e.g. `AuthenticationFailed`)
//! is kept for the UI ([`ffmpeg::AvioSource::take_error`],
//! [`mpv::MpvProtocol::take_error`]). Cancel (FFmpeg interrupt callback,
//! mpv `cancel_fn`) makes the next callback fail instead of reading; a read
//! already blocked in the backend finishes with the backend's own timeout.
//! A panic in a source is caught and reported as an error, never unwound
//! into C.

pub mod ffmpeg;
pub mod mpv;
mod stream;
