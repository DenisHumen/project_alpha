// SPDX-License-Identifier: MPL-2.0
//! Auto engine selection over the built-in capability matrix.

use pa_playback::{
    AudioCodec as A, Container as C, DynamicRange as R, EngineKind as E, EnginePreference,
    Limitation, MediaProfile, Platform as P, RangeSupport, SelectError, SubtitleFormat as S,
    VideoCodec as V, auto_chain, default_candidates, default_capabilities, select_engine,
};

fn profile(container: C, video: V, range: R, audio: &[A], subs: &[S]) -> MediaProfile {
    MediaProfile {
        container,
        video_codec: Some(video),
        dynamic_range: range,
        dv_base_layer: match range {
            R::DolbyVision7 | R::DolbyVision8 => Some(R::Hdr10),
            _ => None,
        },
        audio_codecs: audio.to_vec(),
        subtitle_formats: subs.to_vec(),
    }
}

fn auto(platform: P, p: &MediaProfile) -> E {
    select_engine(&default_candidates(platform), p, EnginePreference::Auto)
        .expect("an engine is selected")
        .engine
}

/// The regression matrix: (platform, file, expected engine).
#[test]
fn auto_matrix() {
    let mp4_sdr = profile(C::Mp4, V::H264, R::Sdr, &[A::Aac], &[S::MovText]);
    let mkv_hdr = profile(
        C::Mkv,
        V::Hevc,
        R::Hdr10,
        &[A::Eac3, A::TrueHd],
        &[S::Pgs, S::Ass],
    );
    let mkv_dv7 = profile(C::Mkv, V::Hevc, R::DolbyVision7, &[A::TrueHd], &[S::Pgs]);
    let mkv_dv8 = profile(C::Mkv, V::Hevc, R::DolbyVision8, &[A::Eac3], &[S::Srt]);
    let hls_dv5 = profile(C::Hls, V::Hevc, R::DolbyVision5, &[A::Eac3], &[S::WebVtt]);
    let avi_divx = profile(C::Avi, V::Mpeg4, R::Sdr, &[A::Mp3], &[]);
    let wmv_vc1 = profile(C::Other, V::Vc1, R::Sdr, &[A::Other], &[]);
    let cases = [
        (P::AndroidTv, &mp4_sdr, E::Media3),
        (P::AndroidTv, &mkv_hdr, E::Media3),
        // P7 is not in the default Media3 list: HDR10 base layer on Media3 beats
        // a tone-mapped DV on libmpv.
        (P::AndroidTv, &mkv_dv7, E::Media3),
        (P::Android, &avi_divx, E::Mpv),
        (P::Android, &wmv_vc1, E::Mpv),
        (P::TvOs, &mp4_sdr, E::AvFoundation),
        (P::TvOs, &hls_dv5, E::AvFoundation),
        (P::TvOs, &mkv_hdr, E::VideoToolbox),
        (P::TvOs, &mkv_dv7, E::VideoToolbox),
        (P::Ios, &mkv_dv8, E::VideoToolbox),
        (P::MacOs, &avi_divx, E::VideoToolbox),
        (P::Linux, &mkv_dv8, E::Mpv),
        (P::Windows, &mkv_hdr, E::Mpv),
    ];
    for (platform, file, expected) in cases {
        assert_eq!(auto(platform, file), expected, "{platform:?} {file:?}");
    }
}

#[test]
fn chains_follow_adr_0003() {
    assert_eq!(auto_chain(P::Android), [E::Media3, E::Mpv]);
    assert_eq!(
        auto_chain(P::TvOs),
        [E::AvFoundation, E::VideoToolbox, E::Mpv]
    );
    assert_eq!(auto_chain(P::Linux), [E::Mpv]);
    for platform in [
        P::Android,
        P::AndroidTv,
        P::Ios,
        P::TvOs,
        P::MacOs,
        P::Windows,
        P::Linux,
    ] {
        let engines: Vec<E> = default_candidates(platform)
            .iter()
            .map(|c| c.engine)
            .collect();
        assert_eq!(engines, auto_chain(platform), "{platform:?}");
    }
    assert!(default_capabilities(E::Media3, P::Ios).is_none());
    assert!(default_capabilities(E::AvFoundation, P::Android).is_none());
}

#[test]
fn first_full_engine_wins_and_limitations_are_explained() {
    let file = profile(C::Mkv, V::Hevc, R::DolbyVision7, &[A::TrueHd], &[S::Pgs]);
    let sel = select_engine(
        &default_candidates(P::AndroidTv),
        &file,
        EnginePreference::Auto,
    )
    .expect("selected");
    assert!(!sel.manual);
    assert_eq!(
        sel.chosen().limitations,
        [Limitation::BaseLayerOnly {
            range: R::DolbyVision7,
            shown_as: R::Hdr10
        }]
    );
    let mpv = &sel.evaluations[1];
    assert_eq!(mpv.engine, E::Mpv);
    assert_eq!(
        mpv.limitations,
        [Limitation::ToneMapped {
            range: R::DolbyVision7
        }]
    );

    // A refined matrix (this SoC decodes P7) makes Media3 full.
    let mut candidates = default_candidates(P::AndroidTv);
    candidates[0]
        .dynamic_ranges
        .push(pa_playback::RangeCapability {
            range: R::DolbyVision7,
            support: RangeSupport::Native,
        });
    let sel = select_engine(&candidates, &file, EnginePreference::Auto).expect("selected");
    assert!(sel.chosen().is_full());
}

#[test]
fn partial_audio_and_missing_audio() {
    let caps = default_capabilities(E::AvFoundation, P::TvOs).expect("caps");
    let two = profile(C::Mp4, V::Hevc, R::Sdr, &[A::Eac3, A::TrueHd], &[]);
    let eval = pa_playback::evaluate(&caps, &two);
    assert!(eval.is_playable() && !eval.is_full());
    assert_eq!(
        eval.limitations,
        [Limitation::AudioCodecUnsupported { codec: A::TrueHd }]
    );
    let none = profile(C::Mp4, V::Hevc, R::Sdr, &[A::TrueHd, A::Dts], &[]);
    assert!(!pa_playback::evaluate(&caps, &none).is_playable());
}

#[test]
fn dv5_without_dv_support_is_blocking() {
    // No base layer: wrong colors without DV support.
    let mut caps = default_capabilities(E::Media3, P::AndroidTv).expect("caps");
    caps.dynamic_ranges.retain(|r| !r.range.is_dolby_vision());
    let file = profile(C::Mkv, V::Hevc, R::DolbyVision5, &[A::Eac3], &[]);
    let eval = pa_playback::evaluate(&caps, &file);
    assert_eq!(
        eval.limitations,
        [Limitation::DynamicRangeUnsupported {
            range: R::DolbyVision5
        }]
    );
    // Auto then falls back to libmpv.
    let mut candidates = default_candidates(P::AndroidTv);
    candidates[0] = caps;
    assert_eq!(
        select_engine(&candidates, &file, EnginePreference::Auto)
            .expect("selected")
            .engine,
        E::Mpv
    );
}

#[test]
fn manual_override() {
    let file = profile(C::Mkv, V::Hevc, R::Hdr10, &[A::Eac3], &[]);
    let candidates = default_candidates(P::TvOs);
    let sel = select_engine(&candidates, &file, EnginePreference::Manual(E::Mpv)).expect("mpv");
    assert_eq!((sel.engine, sel.manual), (E::Mpv, true));
    // Honoured even when the matrix says the engine cannot play the file.
    let sel = select_engine(
        &candidates,
        &file,
        EnginePreference::Manual(E::AvFoundation),
    )
    .expect("manual");
    assert_eq!(sel.engine, E::AvFoundation);
    assert!(!sel.chosen().is_playable());
    assert_eq!(
        select_engine(&candidates, &file, EnginePreference::Manual(E::Media3)),
        Err(SelectError::EngineNotAvailable { engine: E::Media3 })
    );
}

#[test]
fn nothing_playable_and_no_candidates() {
    let file = profile(C::Avi, V::Mpeg4, R::Sdr, &[A::Mp3], &[]);
    let candidates = vec![default_capabilities(E::AvFoundation, P::TvOs).expect("caps")];
    assert!(matches!(
        select_engine(&candidates, &file, EnginePreference::Auto),
        Err(SelectError::NoPlayableEngine { evaluations }) if evaluations.len() == 1
    ));
    assert_eq!(
        select_engine(&[], &file, EnginePreference::Auto),
        Err(SelectError::NoCandidates)
    );
}

#[test]
fn audio_only_files_ignore_video_rules() {
    let flac = MediaProfile {
        container: C::Other,
        audio_codecs: vec![A::Flac],
        ..MediaProfile::default()
    };
    assert_eq!(auto(P::Linux, &flac), E::Mpv);
}

#[test]
fn preference_serializes_for_settings() {
    let manual = serde_json::to_string(&EnginePreference::Manual(E::VideoToolbox)).expect("json");
    assert_eq!(manual, r#"{"mode":"manual","engine":"video_toolbox"}"#);
    let auto = serde_json::to_string(&EnginePreference::Auto).expect("json");
    assert_eq!(auto, r#"{"mode":"auto"}"#);
}
