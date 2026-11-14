// SPDX-License-Identifier: MPL-2.0
//! `pa-probe` against the QA-002 test media (`tests/media/manifest.toml`).
//!
//! Needs our LGPL FFmpeg build and the generated media set:
//!
//! ```text
//! just tests::media                                   # tests/media/synthetic
//! PA_FFMPEG_LIB_DIR=tools/ffmpeg/out/linux-x86_64/lib cargo test -p pa-probe --test media
//! ```
//!
//! `PA_TEST_MEDIA_DIR` overrides the media root (default `tests/media`).
//! Without FFmpeg or the media the tests are skipped.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pa_library::rusqlite::params;
use pa_probe::{
    Ffmpeg, HdrFormat, MediaInfo, ProbeErrorKind, ProbeOptions, StreamCommon, SubtitleKind, store,
};
use pa_vfs::{ByteSource, LocalFs, Vfs, VfsPath};
use toml::Value;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn media_root() -> PathBuf {
    std::env::var_os("PA_TEST_MEDIA_DIR")
        .map_or_else(|| repo_root().join("tests/media"), PathBuf::from)
}

/// FFmpeg and the synthetic media set, or `None` (test skipped).
fn setup() -> Option<(Ffmpeg, PathBuf)> {
    if std::env::var_os(pa_probe::LIB_DIR_ENV).is_none() {
        eprintln!("skipped: {} not set", pa_probe::LIB_DIR_ENV);
        return None;
    }
    let ffmpeg = Ffmpeg::load_default().expect("FFmpeg from PA_FFMPEG_LIB_DIR");
    let root = media_root();
    if !root.join("synthetic").is_dir() {
        eprintln!(
            "skipped: no {}/synthetic (run `just tests::media`)",
            root.display()
        );
        return None;
    }
    Some((ffmpeg, root))
}

fn manifest() -> Vec<Value> {
    let text = std::fs::read_to_string(repo_root().join("tests/media/manifest.toml")).unwrap();
    let doc: Value = toml::from_str(&text).unwrap();
    doc["file"].as_array().unwrap().clone()
}

fn source(root: &Path, rel: &str) -> Arc<dyn ByteSource> {
    let vfs = LocalFs::new(root).unwrap();
    Arc::from(
        vfs.open(&VfsPath::parse(&format!("/{rel}")).unwrap())
            .unwrap(),
    )
}

/// One stream of any type, for uniform checks.
enum Stream<'a> {
    Video(&'a pa_probe::VideoStream),
    Audio(&'a pa_probe::AudioStream),
    Subtitle(&'a pa_probe::SubtitleStream),
    Attachment(&'a pa_probe::Attachment),
}

impl Stream<'_> {
    fn common(&self) -> Option<&StreamCommon> {
        match self {
            Self::Video(v) => Some(&v.common),
            Self::Audio(a) => Some(&a.common),
            Self::Subtitle(s) => Some(&s.common),
            Self::Attachment(_) => None,
        }
    }
}

fn streams_of<'a>(info: &'a MediaInfo, kind: &str) -> Vec<Stream<'a>> {
    match kind {
        "video" => info.video.iter().map(Stream::Video).collect(),
        "audio" => info.audio.iter().map(Stream::Audio).collect(),
        "subtitle" => info.subtitles.iter().map(Stream::Subtitle).collect(),
        "attachment" => info.attachments.iter().map(Stream::Attachment).collect(),
        _ => Vec::new(),
    }
}

/// Compares `info` with a manifest entry; returns the mismatches.
fn check(entry: &Value, info: &MediaInfo) -> Vec<String> {
    let mut errs = Vec::new();
    let mut expect_eq = |what: String, want: String, got: String| {
        if want != got {
            errs.push(format!("{what}: want {want}, got {got}"));
        }
    };
    let empty = Value::Table(toml::Table::new());
    let expect = entry.get("expect").unwrap_or(&empty);

    if let Some(format) = expect.get("format").and_then(Value::as_str) {
        expect_eq("format".into(), format.into(), info.container.clone());
    }
    if let Some(want) = expect.get("duration").and_then(Value::as_float) {
        let got = info.duration_ms.map(|ms| ms as f64 / 1000.0);
        let ok = got.is_some_and(|g| (g - want).abs() <= (want * 0.03).max(0.15));
        if !ok {
            expect_eq("duration".into(), format!("{want}"), format!("{got:?}"));
        }
    }
    if let Some(n) = expect.get("chapters").and_then(Value::as_integer) {
        expect_eq(
            "chapters".into(),
            n.to_string(),
            info.chapters.len().to_string(),
        );
    }
    if let Some(titles) = expect.get("chapter_titles").and_then(Value::as_array) {
        let want: Vec<_> = titles.iter().filter_map(Value::as_str).collect();
        let got: Vec<_> = info
            .chapters
            .iter()
            .map(|c| c.title.as_deref().unwrap_or(""))
            .collect();
        expect_eq(
            "chapter titles".into(),
            format!("{want:?}"),
            format!("{got:?}"),
        );
    }
    if let Some(tags) = expect.get("tags").and_then(Value::as_table) {
        for (k, v) in tags {
            let got = info
                .tags
                .get(&k.to_lowercase())
                .cloned()
                .unwrap_or_default();
            expect_eq(format!("tag {k}"), v.as_str().unwrap_or("").into(), got);
        }
    }

    let mut taken = std::collections::HashMap::<String, usize>::new();
    for s in entry
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = s["type"].as_str().unwrap();
        let k = taken.entry(kind.to_owned()).or_default();
        let all = streams_of(info, kind);
        let Some(got) = all.get(*k) else {
            errs.push(format!("{kind} #{k}: missing (have {})", all.len()));
            continue;
        };
        *k += 1;
        errs.extend(
            check_stream(s, got)
                .into_iter()
                .map(|e| format!("{kind} #{}: {e}", *k - 1)),
        );
    }
    errs
}

fn check_stream(want: &Value, got: &Stream<'_>) -> Vec<String> {
    let mut errs = Vec::new();
    let s = |k: &str| want.get(k).and_then(Value::as_str);
    let i = |k: &str| want.get(k).and_then(Value::as_integer);
    let b = |k: &str| want.get(k).and_then(Value::as_bool);
    let mut eq = |what: &str, w: String, g: String| {
        if w != g {
            errs.push(format!("{what}: want {w}, got {g}"));
        }
    };
    if let Some(c) = got.common() {
        if let Some(w) = s("codec") {
            eq("codec", w.into(), c.codec.clone());
        }
        if let Some(w) = s("profile") {
            eq("profile", w.into(), c.profile.clone().unwrap_or_default());
        }
        if let Some(w) = s("lang") {
            eq("lang", w.into(), c.language.clone().unwrap_or_default());
        }
        if let Some(w) = s("title") {
            eq("title", w.into(), c.title.clone().unwrap_or_default());
        }
        for (key, flag) in [
            ("default", c.default),
            ("forced", c.forced),
            ("hearing_impaired", c.hearing_impaired),
            ("comment", c.comment),
        ] {
            if let Some(w) = b(key) {
                eq(key, w.to_string(), flag.to_string());
            }
        }
    }
    match got {
        Stream::Video(v) => {
            if let Some(w) = i("width") {
                eq("width", w.to_string(), v.width.to_string());
            }
            if let Some(w) = i("height") {
                eq("height", w.to_string(), v.height.to_string());
            }
            if let Some(w) = s("pix_fmt") {
                eq("pix_fmt", w.into(), v.pix_fmt.clone().unwrap_or_default());
            }
            if let Some(w) = s("codec_tag") {
                eq(
                    "codec_tag",
                    w.into(),
                    v.codec_tag.clone().unwrap_or_default(),
                );
            }
            if let Some(w) = s("fps") {
                let got = v
                    .frame_rate
                    .map(|(n, d)| format!("{n}/{d}"))
                    .unwrap_or_default();
                eq("fps", w.into(), got);
            }
            if let Some(w) = s("sar") {
                let got = v
                    .sample_aspect_ratio
                    .map(|(n, d)| format!("{n}:{d}"))
                    .unwrap_or_default();
                eq("sar", w.into(), got);
            }
            if let Some(w) = s("color_transfer") {
                eq(
                    "color_transfer",
                    w.into(),
                    v.color_transfer.clone().unwrap_or_default(),
                );
            }
            if let Some(w) = s("color_primaries") {
                eq(
                    "color_primaries",
                    w.into(),
                    v.color_primaries.clone().unwrap_or_default(),
                );
            }
            if let Some(w) = s("field_order") {
                eq(
                    "interlaced",
                    (w != "progressive").to_string(),
                    format!("{}", v.interlaced == Some(true)),
                );
            }
            if let Some(w) = b("attached_pic") {
                eq("cover_art", w.to_string(), v.cover_art.to_string());
            }
            let dv = v.dolby_vision.unwrap_or_default();
            if let Some(w) = i("dv_profile") {
                eq(
                    "dv_profile",
                    w.to_string(),
                    format!("{:?}", dv.profile.map(i64::from).unwrap_or(-1)),
                );
                eq(
                    "hdr[0]",
                    "Some(DolbyVision)".into(),
                    format!("{:?}", v.hdr_format()),
                );
            }
            if let Some(w) = i("dv_compat") {
                eq(
                    "dv_compat",
                    w.to_string(),
                    format!("{}", dv.compatibility_id.map_or(-1, i64::from)),
                );
            }
            let frame_sd: Vec<&str> = want
                .get("frame_side_data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if frame_sd.iter().any(|x| x.contains("HDR10+")) {
                eq(
                    "hdr10+",
                    "true".into(),
                    v.hdr.contains(&HdrFormat::Hdr10Plus).to_string(),
                );
            }
            if frame_sd.iter().any(|x| x.contains("Dolby Vision RPU")) {
                eq("dv rpu", "true".into(), dv.rpu.to_string());
            }
            match v.color_transfer.as_deref() {
                Some("smpte2084") if !v.hdr.contains(&HdrFormat::Hdr10) => {
                    errs.push(format!("PQ stream without Hdr10: {:?}", v.hdr));
                }
                Some("arib-std-b67") if !v.hdr.contains(&HdrFormat::Hlg) => {
                    errs.push(format!("HLG stream without Hlg: {:?}", v.hdr));
                }
                _ => {}
            }
        }
        Stream::Audio(a) => {
            if let Some(w) = i("channels") {
                eq("channels", w.to_string(), a.channels.to_string());
            }
            if let Some(w) = i("sample_rate") {
                eq(
                    "sample_rate",
                    w.to_string(),
                    a.sample_rate.unwrap_or(0).to_string(),
                );
            }
            if let Some(w) = i("bits") {
                eq("bits", w.to_string(), a.bit_depth.unwrap_or(0).to_string());
            }
        }
        Stream::Subtitle(sub) => {
            let bitmap = ["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle"];
            let want = if bitmap.contains(&sub.common.codec.as_str()) {
                SubtitleKind::Bitmap
            } else {
                SubtitleKind::Text
            };
            eq("kind", format!("{want:?}"), format!("{:?}", sub.kind));
        }
        Stream::Attachment(att) => {
            if let Some(w) = s("filename") {
                eq(
                    "filename",
                    w.into(),
                    att.filename.clone().unwrap_or_default(),
                );
            }
            if let Some(w) = s("mimetype") {
                eq(
                    "mimetype",
                    w.into(),
                    att.mime_type.clone().unwrap_or_default(),
                );
            }
            if att.size_bytes.unwrap_or(0) == 0 {
                errs.push("attachment without data".into());
            }
        }
    }
    errs
}

#[test]
fn synthetic_media_match_the_manifest() {
    let Some((ffmpeg, root)) = setup() else {
        return;
    };
    let opts = ProbeOptions::default();
    let (mut checked, mut lib006, mut failures) = (0, 0, Vec::new());
    for entry in manifest() {
        let rel = entry["path"].as_str().unwrap();
        if !rel.starts_with("synthetic/") || !root.join(rel).is_file() {
            continue;
        }
        let id = entry["id"].as_str().unwrap();
        let used_by_us = entry["used_by"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("LIB-006")));
        let want_error = entry
            .get("expect")
            .and_then(|e| e.get("probe"))
            .and_then(Value::as_str)
            == Some("error");
        checked += 1;
        lib006 += usize::from(used_by_us);
        match (ffmpeg.probe(source(&root, rel), &opts), want_error) {
            (Ok(info), false) => {
                for e in check(&entry, &info) {
                    failures.push(format!("{id}: {e}"));
                }
            }
            (Err(e), true) => assert_eq!(e.kind(), ProbeErrorKind::InvalidData, "{id}: {e}"),
            (Ok(info), true) => failures.push(format!("{id}: expected an error, got {info:?}")),
            (Err(e), false) => failures.push(format!("{id}: {e}")),
        }
    }
    eprintln!("checked {checked} files ({lib006} used by LIB-006)");
    assert!(checked >= 80, "only {checked} files found");
    assert!(lib006 >= 8, "only {lib006} LIB-006 files found");
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Spot checks of fields the manifest does not describe.
#[test]
fn hdr_and_track_details() {
    let Some((ffmpeg, root)) = setup() else {
        return;
    };
    let opts = ProbeOptions::default();
    let probe = |rel: &str| {
        ffmpeg
            .probe(source(&root, &format!("synthetic/{rel}")), &opts)
            .unwrap()
    };

    let dv = probe("hdr/dv-p8.1-hevc-1080p.mkv");
    let v = dv.main_video().unwrap();
    assert_eq!(v.hdr, [HdrFormat::DolbyVision, HdrFormat::Hdr10]);
    let cfg = v.dolby_vision.unwrap();
    assert_eq!(cfg.profile_name().as_deref(), Some("8.1"));
    assert!(cfg.rpu && cfg.base_layer && !cfg.enhancement_layer);
    assert_eq!(v.bit_depth, Some(10));
    assert!(v.mastering_display);

    let p84 = probe("hdr/dv-p8.4-hlg-hevc-1080p.mp4");
    assert_eq!(
        p84.main_video().unwrap().hdr,
        [HdrFormat::DolbyVision, HdrFormat::Hlg]
    );

    let p5 = probe("hdr/dv-p5-hevc-1080p.mp4");
    assert_eq!(p5.main_video().unwrap().hdr, [HdrFormat::DolbyVision]);

    let plus = probe("hdr/hdr10plus-hevc-1080p.mkv");
    assert_eq!(
        plus.main_video().unwrap().hdr,
        [HdrFormat::Hdr10Plus, HdrFormat::Hdr10]
    );
    let plus_mp4 = probe("hdr/hdr10plus-hevc-1080p.mp4");
    assert_eq!(
        plus_mp4.main_video().unwrap().hdr,
        [HdrFormat::Hdr10Plus, HdrFormat::Hdr10]
    );

    let hdr10 = probe("hdr/hdr10-hevc-1080p.mkv");
    let v = hdr10.main_video().unwrap();
    assert_eq!(v.hdr, [HdrFormat::Hdr10]);
    assert!(v.mastering_display && v.content_light_level);

    let sdr = probe("video/h264-aac.mp4");
    assert!(sdr.main_video().unwrap().hdr.is_empty());
    assert!(sdr.duration_source.is_some());

    let multi = probe("tracks/multi-audio-subs-chapters.mkv");
    assert_eq!(multi.audio_languages(), ["eng", "rus", "jpn"]);
    assert_eq!(multi.subtitle_languages(), ["eng", "rus"]);
    assert_eq!(multi.audio[1].channel_layout.as_deref(), Some("5.1(side)"));
    assert!(
        multi
            .chapters
            .windows(2)
            .all(|w| w[0].end_ms <= w[1].start_ms)
    );
    assert!(!multi.window_truncated);

    let cover = probe("music/formats/id3v2-cover.mp3");
    assert!(cover.video.iter().all(|v| v.cover_art));
    assert!(cover.main_video().is_none());
}

/// A tight window still identifies the streams and never reads past it.
#[test]
fn reads_stay_within_the_window() {
    let Some((ffmpeg, root)) = setup() else {
        return;
    };
    let tight = ProbeOptions {
        head_bytes: 1 << 20,
        tail_bytes: 512 << 10,
        ..ProbeOptions::default()
    };
    let (mut big, mut truncated) = (0, 0);
    for entry in manifest() {
        let rel = entry["path"].as_str().unwrap();
        let path = root.join(rel);
        if !rel.starts_with("synthetic/") || !path.is_file() {
            continue;
        }
        let size = std::fs::metadata(&path).unwrap().len();
        let Ok(info) = ffmpeg.probe(source(&root, rel), &tight) else {
            continue;
        };
        assert!(
            info.bytes_read <= (1 << 20) + (512 << 10),
            "{rel}: read {} bytes",
            info.bytes_read
        );
        if size > 3 << 20 {
            big += 1;
            let full = ffmpeg
                .probe(source(&root, rel), &ProbeOptions::default())
                .unwrap();
            assert_eq!(info.container, full.container, "{rel}");
            assert_eq!(
                info.main_video()
                    .map(|v| (&v.common.codec, v.width, v.height)),
                full.main_video()
                    .map(|v| (&v.common.codec, v.width, v.height)),
                "{rel}"
            );
            truncated += usize::from(info.window_truncated);
        }
    }
    assert!(big > 0, "no file larger than the window");
    // libavformat stops early when it has what it needs; some files make it
    // read on and hit the window.
    assert!(truncated > 0, "no probe was cut by the window");
}

#[test]
fn cancelled_probe_reports_cancelled() {
    let Some((ffmpeg, root)) = setup() else {
        return;
    };
    let opts = ProbeOptions {
        cancel: Some(Arc::new(AtomicBool::new(true))),
        ..ProbeOptions::default()
    };
    let err = ffmpeg
        .probe(source(&root, "synthetic/video/h264-aac.mp4"), &opts)
        .unwrap_err();
    assert_eq!(err.kind(), ProbeErrorKind::Cancelled);
}

#[test]
fn probe_pending_fills_the_library() {
    let Some((ffmpeg, root)) = setup() else {
        return;
    };
    let conn = pa_library::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'Media', 'file:///media', 0)",
        [],
    )
    .unwrap();
    for (id, path) in [
        (1, "/video/h264-aac.mp4"),
        (2, "/tracks/multi-audio-subs-chapters.mkv"),
        (3, "/edge/zero-bytes.mkv"),
        (4, "/edge/does-not-exist.mkv"),
        (5, "/music/formats/id3v2-cover.mp3"),
    ] {
        conn.execute(
            "INSERT INTO files (id, source_id, path, size_bytes, added_at_ms) VALUES (?1, 1, ?2, 1, 0)",
            params![id, path],
        )
        .unwrap();
    }
    let vfs = LocalFs::new(root.join("synthetic")).unwrap();
    let video_only = |p: &str| !p.ends_with(".mp3");
    let report = store::probe_pending(
        &conn,
        1,
        &vfs,
        &ffmpeg,
        &ProbeOptions::default(),
        &video_only,
        100,
    )
    .unwrap();
    assert_eq!(report.probed, 2);
    assert_eq!(report.invalid, 2);
    assert_eq!(report.retry_later, 0);

    let (duration, audio, subs): (i64, String, String) = conn
        .query_row(
            "SELECT duration_ms, audio_langs, sub_langs FROM files WHERE id = 2",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!((4900..=5100).contains(&duration), "{duration}");
    assert_eq!(audio, r#"["eng","rus","jpn"]"#);
    assert_eq!(subs, r#"["eng","rus"]"#);
    // The mp3 was filtered out and stays pending.
    assert_eq!(
        store::pending_files(&conn, 1, 10).unwrap(),
        [(5, "/music/formats/id3v2-cover.mp3".to_owned())]
    );
}
