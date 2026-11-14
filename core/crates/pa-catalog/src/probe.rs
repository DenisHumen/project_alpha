// SPDX-License-Identifier: MPL-2.0
//! Container probe of new and changed files in the scan pipeline (LIB-006).
//!
//! [`pa_probe::store::probe_pending`] reads every file the scan left
//! unprobed through the source's [`Vfs`] (local, SMB, WebDAV alike) and
//! fills `duration_ms`, `container`, `video_json`, `audio_langs`,
//! `sub_langs` and `audio_formats`. Recognition uses the duration, the
//! library queries (LIB-009) the video and audio columns.
//!
//! FFmpeg is loaded at run time; without it the step is skipped and the
//! files stay pending for a later scan.

use pa_library::rusqlite::Connection;
use pa_probe::store::{PendingError, probe_pending};
use pa_probe::{Ffmpeg, ProbeOptions};
use pa_scanner::CancelToken;
use pa_vfs::Vfs;

use crate::Error;

/// Files per batch: cancellation is checked between batches.
const BATCH: usize = 32;

/// Probes the pending files of `source_id`. Returns a message for the user
/// when probing stopped early (cancelled, FFmpeg failed mid-way); `None`
/// when done or FFmpeg is not installed.
pub(crate) fn run(
    conn: &Connection,
    source_id: i64,
    vfs: &dyn Vfs,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<Option<String>, Error> {
    let total: i64 = conn.query_row(
        "SELECT count(*) FROM files
         WHERE source_id = ?1 AND probed_at_ms IS NULL AND missing_since_ms IS NULL",
        [source_id],
        |r| r.get(0),
    )?;
    let total = usize::try_from(total).unwrap_or(0);
    if total == 0 {
        return Ok(None);
    }
    let Ok(ffmpeg) = Ffmpeg::shared() else {
        // No FFmpeg here (tests, a build without it): durations stay empty.
        return Ok(None);
    };
    run_with(conn, source_id, vfs, ffmpeg, cancel, total, on_progress)
}

fn run_with(
    conn: &Connection,
    source_id: i64,
    vfs: &dyn Vfs,
    ffmpeg: &Ffmpeg,
    cancel: &CancelToken,
    total: usize,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<Option<String>, Error> {
    let options = ProbeOptions::default();
    let mut done = 0;
    on_progress(done, total);
    while !cancel.is_cancelled() {
        let report = match probe_pending(
            conn,
            source_id,
            vfs,
            ffmpeg,
            &options,
            &|_| true,
            BATCH,
        ) {
            Ok(r) => r,
            Err(PendingError::Db(e)) => return Err(e.into()),
            Err(PendingError::Probe(e)) => return Ok(Some(format!("probe: {e}"))),
        };
        let progressed = report.probed + report.invalid;
        done += progressed;
        on_progress(done.min(total), total);
        // Only files left for later (source errors that may pass): stop.
        if progressed == 0 {
            break;
        }
    }
    Ok(None)
}
