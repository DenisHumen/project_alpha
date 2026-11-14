// SPDX-License-Identifier: MPL-2.0
//! The scan probes new files (LIB-006 in the pipeline, LIB-018): with
//! FFmpeg (`PA_FFMPEG_LIB_DIR`) durations and audio formats are filled;
//! without it the scan still succeeds and the files stay pending.

use pa_catalog::Catalog;
use pa_scanner::CancelToken;

/// A 2-second 8 kHz mono 16-bit PCM WAV file.
fn wav() -> Vec<u8> {
    let rate: u32 = 8000;
    let data_len: u32 = rate * 2 * 2;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v.extend((0..data_len).map(|i| (i % 7) as u8));
    v
}

fn scan_wav() -> (tempfile::TempDir, Catalog, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let media = dir.path().join("media/Artist/Album");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("01 - Song.wav"), wav()).unwrap();
    let cat = Catalog::open(dir.path().join("library.db")).unwrap();
    cat.add_source(
        None,
        "local",
        "Music",
        dir.path().join("media").to_str().unwrap(),
        None,
    )
    .unwrap();
    let mut errors = Vec::new();
    cat.scan(None, &CancelToken::new(), &mut |u| errors.extend(u.error))
        .unwrap();
    (dir, cat, errors)
}

#[test]
fn the_scan_probes_new_files_when_ffmpeg_is_there() {
    let with_ffmpeg = std::env::var_os(pa_probe::LIB_DIR_ENV).is_some();
    let (_dir, cat, errors) = scan_wav();
    assert_eq!(errors, Vec::<String>::new());
    let conn = pa_library::open(cat.path()).unwrap();
    let (duration, probed, formats): (Option<i64>, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT duration_ms, probed_at_ms, audio_formats FROM files",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    if with_ffmpeg {
        let d = duration.expect("duration probed");
        assert!((1900..=2100).contains(&d), "{d}");
        assert!(probed.is_some());
        assert!(formats.is_some_and(|f| f.starts_with('[')));
    } else if pa_probe::Ffmpeg::shared().is_err() {
        // FFmpeg missing: skipped gracefully, retried by a later scan.
        assert_eq!((duration, probed), (None, None));
        eprintln!("no FFmpeg: checked the skip only");
    }
}
