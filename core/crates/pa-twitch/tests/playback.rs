// SPDX-License-Identifier: MPL-2.0
//! TWI-004: playback decision from the remote config and, in the `full`
//! flavor, token → usher master playlist → variants. Hand-written fixtures
//! with made-up channels and ids; no network.

use std::sync::Arc;

use pa_remote_config::{ClientContext, KillSwitch, PlayerMode, RemoteConfig, flags};
use pa_twitch::playback::{
    DeviceCodecs, EmbedReason, Flavor, PlaybackError, PlaybackTarget, PlayerChoice, TwitchFeatures,
    TwitchPlayback,
};
use pa_twitch::testing::FixtureTransport;

const GQL_ON: &str = include_str!("fixtures/playback/remote-config-gql-on.json");

fn gql_on() -> RemoteConfig {
    RemoteConfig::parse(GQL_ON.as_bytes()).unwrap()
}

fn ctx(version: &str) -> ClientContext {
    ClientContext {
        app_version: version.into(),
        platform: "linux".into(),
    }
}

fn live() -> PlaybackTarget {
    PlaybackTarget::live("Fake_Streamer_One").unwrap()
}

fn kill_up_to(version: &str) -> KillSwitch {
    KillSwitch {
        feature: flags::TWITCH_GQL_PLAYBACK.into(),
        min_app_version: None,
        max_app_version: Some(version.into()),
        platforms: vec![],
        reason: Some("integrity token required".into()),
    }
}

fn full_reason(config: &RemoteConfig, version: &str) -> Option<EmbedReason> {
    match TwitchFeatures::resolve_for(config, &ctx(version), Flavor::Full).player {
        PlayerChoice::Embed(reason) => Some(reason),
        PlayerChoice::Native(_) => None,
    }
}

/// Resolves through the public entry point of this build and checks that
/// the answer is an embed decision made without a single request.
async fn embed_without_network(config: &RemoteConfig, version: &str) -> EmbedReason {
    let transport = Arc::new(FixtureTransport::new());
    let playback = TwitchPlayback::new(
        TwitchFeatures::resolve(config, &ctx(version)),
        transport.clone(),
    );
    let decision = playback
        .resolve(&live(), DeviceCodecs::H264_ONLY)
        .await
        .unwrap();
    assert!(
        transport.requests().is_empty(),
        "embed path must not call Twitch"
    );
    let embed = decision.as_embed().expect("embed decision").clone();
    assert_eq!(embed.target, live());
    embed.reason
}

#[test]
fn builtin_config_keeps_gql_off() {
    let builtin = pa_remote_config::builtin();
    assert_eq!(
        full_reason(&builtin, "0.1.0"),
        Some(EmbedReason::GqlPlaybackOff)
    );
    let f = TwitchFeatures::resolve_for(&builtin, &ctx("0.1.0"), Flavor::Full);
    assert!(!f.chapters_gql && !f.chat_replay && !f.archive);
}

#[test]
fn test_config_enables_native_with_config_values() {
    let f = TwitchFeatures::resolve_for(&gql_on(), &ctx("0.1.0"), Flavor::Full);
    let PlayerChoice::Native(settings) = f.player else {
        panic!("expected native, got {:?}", f.player);
    };
    assert_eq!(settings.client_id, "test-client-id-not-real");
    assert!(settings.playback_access_token_hash.ends_with("0001"));
    assert!(f.chapters_gql && !f.chat_replay && f.archive);
}

#[test]
fn store_flavor_is_always_embed_and_gql_free() {
    let f = TwitchFeatures::resolve_for(&gql_on(), &ctx("0.1.0"), Flavor::Store);
    assert_eq!(f.player, PlayerChoice::Embed(EmbedReason::StoreBuild));
    assert!(!f.chapters_gql && !f.chat_replay && !f.archive);
}

#[test]
fn flags_and_player_mode_select_embed() {
    let mut off = gql_on();
    off.flags.insert(flags::TWITCH_GQL_PLAYBACK.into(), false);
    assert_eq!(
        full_reason(&off, "0.1.0"),
        Some(EmbedReason::GqlPlaybackOff)
    );
    let f = TwitchFeatures::resolve_for(&off, &ctx("0.1.0"), Flavor::Full);
    assert!(!f.chapters_gql, "GQL chapters follow GQL playback");

    let mut no_id = gql_on();
    no_id.twitch.gql_client_id = None;
    assert_eq!(
        full_reason(&no_id, "0.1.0"),
        Some(EmbedReason::GqlPlaybackOff)
    );

    let mut embed = gql_on();
    embed.twitch.player = PlayerMode::Embed;
    assert_eq!(full_reason(&embed, "0.1.0"), Some(EmbedReason::PlayerEmbed));

    let mut no_hash = gql_on();
    no_hash.twitch.persisted_queries.clear();
    assert_eq!(
        full_reason(&no_hash, "0.1.0"),
        Some(EmbedReason::MissingQueryHash)
    );
}

#[test]
fn kill_switch_by_app_version() {
    let mut config = gql_on();
    config.kill_switches.push(kill_up_to("0.4.0"));
    config.validate().unwrap();
    let killed = Some(EmbedReason::Killed {
        reason: Some("integrity token required".into()),
    });
    assert_eq!(full_reason(&config, "0.1.0-alpha.3"), killed);
    assert_eq!(full_reason(&config, "0.4.0"), killed);
    assert_eq!(
        full_reason(&config, "0.4.1"),
        None,
        "newer apps stay native"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn flag_off_and_kill_switch_answer_embed_without_requests() {
    let mut off = gql_on();
    off.flags.insert(flags::TWITCH_GQL_PLAYBACK.into(), false);
    let mut killed = gql_on();
    killed.kill_switches.push(kill_up_to("0.4.0"));
    let builtin = pa_remote_config::builtin();
    if Flavor::CURRENT == Flavor::Store {
        for config in [&off, &killed, &builtin, &gql_on()] {
            assert_eq!(
                embed_without_network(config, "0.1.0").await,
                EmbedReason::StoreBuild
            );
        }
    } else {
        assert_eq!(
            embed_without_network(&off, "0.1.0").await,
            EmbedReason::GqlPlaybackOff
        );
        assert!(matches!(
            embed_without_network(&killed, "0.1.0").await,
            EmbedReason::Killed { .. }
        ));
        assert_eq!(
            embed_without_network(&builtin, "0.1.0").await,
            EmbedReason::GqlPlaybackOff
        );
    }
}

#[test]
fn targets_are_validated() {
    assert_eq!(
        PlaybackTarget::live(" Fake_Streamer_One ").unwrap(),
        PlaybackTarget::Live {
            login: "fake_streamer_one".into()
        }
    );
    assert_eq!(
        PlaybackTarget::vod("v1234567890").unwrap(),
        PlaybackTarget::Vod {
            video_id: "1234567890".into()
        }
    );
    for bad in [
        "",
        "a/b",
        "../x",
        "x?y=1",
        "name with space",
        &"a".repeat(26),
    ] {
        assert!(PlaybackTarget::live(bad).is_err(), "{bad:?}");
    }
    for bad in ["", "v", "12a", "1/2"] {
        assert!(PlaybackTarget::vod(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn supported_codecs_follow_device() {
    assert_eq!(DeviceCodecs::H264_ONLY.supported_codecs(), "h264");
    let all = DeviceCodecs {
        av1: true,
        hevc: true,
    };
    assert_eq!(all.supported_codecs(), "av1,h265,h264");
    let hevc = DeviceCodecs {
        av1: false,
        hevc: true,
    };
    assert_eq!(hevc.supported_codecs(), "h265,h264");
}

#[test]
fn error_codes_and_fallback() {
    assert_eq!(PlaybackError::GqlHashInvalid.code(), "gql_hash_invalid");
    assert_eq!(
        PlaybackError::IntegrityRequired.code(),
        "integrity_required"
    );
    let restricted = PlaybackError::Restricted {
        code: "vod_manifest_restricted".into(),
    };
    assert_eq!(restricted.code(), "usher_403");
    assert!(restricted.falls_back_to_embed());
    assert!(!PlaybackError::Unavailable.falls_back_to_embed());
    assert!(!PlaybackError::Transport("x".into()).falls_back_to_embed());
}

#[cfg(pa_twitch_native)]
mod native {
    use super::*;
    use pa_twitch::playback::{NativePlayback, PlaybackDecision, QualityPreference};
    use pa_twitch::testing::fixture;
    use pa_twitch::{HttpResponse, Method};

    const LIVE_MASTER: &str = include_str!("fixtures/playback/usher_live_master.m3u8");
    const VOD_MASTER: &str = include_str!("fixtures/playback/usher_vod_master.m3u8");

    fn m3u8(text: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: vec![(
                "Content-Type".into(),
                "application/vnd.apple.mpegurl".into(),
            )],
            body: text.as_bytes().to_vec(),
        }
    }

    fn playback(transport: &Arc<FixtureTransport>, config: &RemoteConfig) -> TwitchPlayback {
        TwitchPlayback::new(
            TwitchFeatures::resolve(config, &ctx("0.1.0")),
            transport.clone(),
        )
    }

    async fn native(
        transport: &Arc<FixtureTransport>,
        target: &PlaybackTarget,
        codecs: DeviceCodecs,
    ) -> NativePlayback {
        match playback(transport, &gql_on()).resolve(target, codecs).await {
            Ok(PlaybackDecision::Native(n)) => n,
            other => panic!("expected native playback, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn live_token_then_master_playlist_then_variants() {
        let transport = Arc::new(FixtureTransport::new());
        transport
            .push_fixture(include_str!("fixtures/playback/gql_token_live.json"))
            .push(m3u8(LIVE_MASTER));
        let codecs = DeviceCodecs {
            av1: false,
            hevc: true,
        };
        let n = native(&transport, &live(), codecs).await;

        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        let gql = &requests[0];
        assert_eq!(gql.method, Method::Post);
        assert_eq!(gql.url, "https://gql.twitch.tv/gql");
        assert_eq!(gql.header("client-id"), Some("test-client-id-not-real"));
        let body: serde_json::Value = serde_json::from_slice(gql.body.as_ref().unwrap()).unwrap();
        assert_eq!(body["operationName"], "PlaybackAccessToken");
        assert_eq!(body["variables"]["login"], "fake_streamer_one");
        assert_eq!(body["variables"]["isLive"], true);
        assert_eq!(body["variables"]["isVod"], false);
        assert!(
            body["extensions"]["persistedQuery"]["sha256Hash"]
                .as_str()
                .unwrap()
                .ends_with("0001")
        );

        let usher = &requests[1];
        assert_eq!(usher.method, Method::Get);
        assert_eq!(usher.url, n.master_url);
        assert!(
            n.master_url
                .starts_with("https://usher.ttvnw.net/api/channel/hls/fake_streamer_one.m3u8?")
        );
        assert!(n.master_url.contains("supported_codecs=h265%2Ch264"));
        assert!(
            n.master_url
                .contains("sig=1111aaaa2222bbbb3333cccc4444dddd5555eeee")
        );
        assert!(n.master_url.contains("token=%7B%22adblock%22%3Afalse"));
        assert!(n.master_url.contains("allow_source=true"));
        assert!(n.master_url.contains("allow_audio_only=true"));
        assert_eq!(
            n.access_token.signature,
            "1111aaaa2222bbbb3333cccc4444dddd5555eeee"
        );

        let qualities: Vec<&str> = n
            .playlist
            .variants
            .iter()
            .map(|v| v.quality.as_str())
            .collect();
        assert_eq!(
            qualities,
            ["1080p60", "720p60", "480p", "160p", "audio_only"]
        );
        let source = n.playlist.select(QualityPreference::Source).unwrap();
        assert!(source.is_source && !source.audio_only);
        assert_eq!(source.resolution, Some((1920, 1080)));
        assert_eq!(source.frame_rate, Some(60.0));
        assert_eq!(source.codecs, ["avc1.64002A", "mp4a.40.2"]);
        assert_eq!(
            source.url,
            "https://video-weaver.zz01.example.invalid/v1/playlist/chunked.m3u8"
        );
        assert_eq!(
            n.playlist
                .select(QualityPreference::AtMost(720))
                .unwrap()
                .quality,
            "720p60"
        );
        assert_eq!(
            n.playlist
                .select(QualityPreference::AtMost(100))
                .unwrap()
                .quality,
            "160p",
            "below every variant falls back to the smallest"
        );
        let audio = n.playlist.select(QualityPreference::AudioOnly).unwrap();
        assert!(audio.audio_only && audio.resolution.is_none());
        assert_eq!(audio.bandwidth, 160_000);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn vod_token_then_master_playlist_then_variants() {
        let transport = Arc::new(FixtureTransport::new());
        transport
            .push_fixture(include_str!("fixtures/playback/gql_token_vod.json"))
            .push(m3u8(VOD_MASTER));
        let target = PlaybackTarget::vod("1234567890").unwrap();
        let n = native(&transport, &target, DeviceCodecs::H264_ONLY).await;

        let body: serde_json::Value =
            serde_json::from_slice(transport.requests()[0].body.as_ref().unwrap()).unwrap();
        assert_eq!(body["variables"]["vodID"], "1234567890");
        assert_eq!(body["variables"]["isVod"], true);
        assert_eq!(body["variables"]["login"], "");
        assert!(
            n.master_url
                .starts_with("https://usher.ttvnw.net/vod/1234567890.m3u8?")
        );
        assert!(n.master_url.contains("supported_codecs=h264&"));
        assert_eq!(n.playlist.variants.len(), 3);
        let source = n.playlist.source().unwrap();
        assert_eq!(
            (source.group_id.as_str(), source.quality.as_str()),
            ("chunked", "1080p60")
        );
        assert!(source.is_source);
        let audio = n.playlist.audio_only().unwrap();
        assert_eq!(audio.quality, "Audio Only");
        assert!(audio.url.ends_with("/audio_only/index-dvr.m3u8"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn gql_failures_are_typed_and_fall_back_to_embed() {
        for (file, expected) in [
            (
                include_str!("fixtures/playback/gql_persisted_query_not_found.json"),
                PlaybackError::GqlHashInvalid,
            ),
            (
                include_str!("fixtures/playback/gql_integrity.json"),
                PlaybackError::IntegrityRequired,
            ),
            (
                include_str!("fixtures/playback/gql_token_null.json"),
                PlaybackError::NotFound,
            ),
        ] {
            let transport = Arc::new(FixtureTransport::new());
            transport.push_fixture(file);
            let err = playback(&transport, &gql_on())
                .resolve(&live(), DeviceCodecs::H264_ONLY)
                .await
                .unwrap_err();
            assert_eq!(err, expected);
            assert_eq!(
                transport.requests().len(),
                1,
                "no usher call after a GQL failure"
            );
        }

        let transport = Arc::new(FixtureTransport::new());
        transport.push_fixture(include_str!(
            "fixtures/playback/gql_persisted_query_not_found.json"
        ));
        let decision = playback(&transport, &gql_on())
            .resolve_or_embed(&live(), DeviceCodecs::H264_ONLY)
            .await
            .unwrap();
        assert_eq!(
            decision.as_embed().unwrap().reason,
            EmbedReason::NativeFailed {
                code: "gql_hash_invalid"
            }
        );

        let transport = Arc::new(FixtureTransport::new());
        transport.push(fixture(
            r#"{ "status": 400, "body": { "error": "Bad Request", "status": 400, "message": "The \"Client-ID\" header is invalid." } }"#,
        ));
        let err = playback(&transport, &gql_on())
            .resolve(&live(), DeviceCodecs::H264_ONLY)
            .await
            .unwrap_err();
        assert!(
            matches!(err, PlaybackError::Gql { status: 400, .. }),
            "{err:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn usher_errors_are_passed_through_not_bypassed() {
        let transport = Arc::new(FixtureTransport::new());
        transport
            .push_fixture(include_str!("fixtures/playback/gql_token_vod.json"))
            .push_fixture(include_str!("fixtures/playback/usher_restricted.json"));
        let err = playback(&transport, &gql_on())
            .resolve(
                &PlaybackTarget::vod("1234567890").unwrap(),
                DeviceCodecs::H264_ONLY,
            )
            .await
            .unwrap_err();
        assert_eq!(
            err,
            PlaybackError::Restricted {
                code: "vod_manifest_restricted".into()
            }
        );
        assert_eq!(transport.remaining(), 0);
        assert_eq!(
            transport.requests().len(),
            2,
            "no retry with other parameters"
        );

        let transport = Arc::new(FixtureTransport::new());
        transport
            .push_fixture(include_str!("fixtures/playback/gql_token_live.json"))
            .push_fixture(include_str!("fixtures/playback/usher_offline.json"));
        let err = playback(&transport, &gql_on())
            .resolve_or_embed(&live(), DeviceCodecs::H264_ONLY)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            PlaybackError::Unavailable,
            "offline is not an embed case"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn kill_switch_by_version_switches_native_off_without_release() {
        let mut config = gql_on();
        config.kill_switches.push(kill_up_to("0.4.0"));

        let transport = Arc::new(FixtureTransport::new());
        let old = TwitchPlayback::new(
            TwitchFeatures::resolve(&config, &ctx("0.3.9")),
            transport.clone(),
        );
        let decision = old.resolve(&live(), DeviceCodecs::H264_ONLY).await.unwrap();
        assert!(matches!(
            decision.as_embed().unwrap().reason,
            EmbedReason::Killed { .. }
        ));
        assert!(transport.requests().is_empty());

        transport
            .push_fixture(include_str!("fixtures/playback/gql_token_live.json"))
            .push(m3u8(LIVE_MASTER));
        let new = TwitchPlayback::new(
            TwitchFeatures::resolve(&config, &ctx("0.5.0")),
            transport.clone(),
        );
        let decision = new.resolve(&live(), DeviceCodecs::H264_ONLY).await.unwrap();
        assert!(matches!(decision, PlaybackDecision::Native(_)));
    }
}
