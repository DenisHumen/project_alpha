// SPDX-License-Identifier: MPL-2.0
//! Twitch playback decision and native HLS resolution (TWI-004).
//!
//! The app asks [`TwitchPlayback::resolve`] how to play a live channel or a
//! VOD and gets a [`PlaybackDecision`]:
//!
//! - [`PlaybackDecision::Embed`]: use the official embed player (TWI-005).
//!   Always the answer in store builds (ADR-0011 §5), and in full builds
//!   whenever the signed remote config (CORE-014, ADR-0039) turns
//!   `twitch.gql_playback` off, kills it for this app version or platform,
//!   asks for `player = embed`, or lacks the persisted-query hash. No request
//!   to Twitch is made on this path.
//! - `PlaybackDecision::Native` (full builds only, `cfg(pa_twitch_native)`): the
//!   `PlaybackAccessToken` from GQL, the usher master playlist URL and its
//!   parsed variants (quality, `audio_only`).
//!
//! # Policy
//!
//! - The GQL client-id and persisted-query hashes come only from the remote
//!   config; nothing of the kind is compiled in. The built-in config keeps
//!   GQL off.
//! - Ads are played as Twitch serves them (06 §2.4, TWI-006): the request
//!   uses the regular web-player values and the playlist from usher is
//!   passed through untouched. No ad blocking, no DRM or sub-only bypass.
//! - The GQL, usher and HLS code is compiled only into full builds: the
//!   `twitch-native` capability of `pa-flavor` (CORE-013), enabled by
//!   `--features pa-flavor/full` or an artifact crate's `full` feature.

#[cfg(pa_twitch_native)]
mod gql;
#[cfg(pa_twitch_native)]
mod hls;
#[cfg(pa_twitch_native)]
mod usher;

use std::sync::Arc;

use pa_remote_config::{ClientContext, PlayerMode, RemoteConfig, flags};

use crate::transport::Transport;

#[cfg(pa_twitch_native)]
pub use gql::PlaybackAccessToken;
#[cfg(pa_twitch_native)]
pub use hls::{MasterPlaylist, QualityPreference, Variant};
#[cfg(pa_twitch_native)]
pub use usher::usher_master_url;

/// Name of the GQL operation whose persisted-query hash the remote config
/// must carry in `twitch.persisted_queries` for native playback.
pub const PLAYBACK_ACCESS_TOKEN_OPERATION: &str = "PlaybackAccessToken";

/// Build flavor of this binary (ADR-0007, ADR-0011 §5): [`Flavor::CURRENT`]
/// is the workspace flavor of `pa-flavor` (CORE-013).
pub use pa_flavor::Flavor;

/// What to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackTarget {
    /// A live channel by login (lowercase).
    Live {
        /// Channel login.
        login: String,
    },
    /// A VOD (archive, highlight, upload) by numeric id.
    Vod {
        /// Video id.
        video_id: String,
    },
}

impl PlaybackTarget {
    /// A live channel. Logins are 1–25 characters of `[a-z0-9_]`
    /// (case-insensitive); anything else is rejected before it reaches a URL.
    pub fn live(login: &str) -> Result<Self, PlaybackError> {
        let login = login.trim().to_ascii_lowercase();
        let valid = (1..=25).contains(&login.len())
            && login
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !valid {
            return Err(PlaybackError::InvalidTarget("bad channel login"));
        }
        Ok(Self::Live { login })
    }

    /// A VOD. Ids are decimal digits (a leading `v` is accepted).
    pub fn vod(video_id: &str) -> Result<Self, PlaybackError> {
        let id = video_id.trim().trim_start_matches('v');
        if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(PlaybackError::InvalidTarget("bad video id"));
        }
        Ok(Self::Vod {
            video_id: id.to_owned(),
        })
    }
}

/// Codecs the device can decode, for usher's `supported_codecs`.
/// H.264 is always included as the universal fallback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceCodecs {
    /// AV1 hardware/software decoding is available.
    pub av1: bool,
    /// HEVC (H.265) decoding is available.
    pub hevc: bool,
}

impl DeviceCodecs {
    /// H.264 only: safe on every device.
    pub const H264_ONLY: DeviceCodecs = DeviceCodecs {
        av1: false,
        hevc: false,
    };

    /// Comma-separated list, best first (`av1,h265,h264`).
    pub fn supported_codecs(&self) -> String {
        let mut codecs = Vec::with_capacity(3);
        if self.av1 {
            codecs.push("av1");
        }
        if self.hevc {
            codecs.push("h265");
        }
        codecs.push("h264");
        codecs.join(",")
    }
}

/// Why the embed player was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedReason {
    /// This is a store build: native playback is not compiled in.
    StoreBuild,
    /// `twitch.gql_playback` is off or the config has no GQL client-id.
    GqlPlaybackOff,
    /// A kill-switch turned GQL playback off for this app version/platform.
    Killed {
        /// The kill-switch's reason, for diagnostics.
        reason: Option<String>,
    },
    /// The config asks for the embed player (`twitch.player = embed`).
    PlayerEmbed,
    /// The config has no `PlaybackAccessToken` persisted-query hash.
    MissingQueryHash,
    /// Native resolution failed in a way the embed may survive
    /// (see [`PlaybackError::falls_back_to_embed`]).
    NativeFailed {
        /// [`PlaybackError::code`] of the failure.
        code: &'static str,
    },
}

/// GQL values from the remote config needed for native playback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GqlSettings {
    /// Public `Client-ID` header for `gql.twitch.tv`.
    pub client_id: String,
    /// Persisted-query hash of [`PLAYBACK_ACCESS_TOKEN_OPERATION`].
    pub playback_access_token_hash: String,
}

/// Which player the config allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerChoice {
    /// Official embed.
    Embed(EmbedReason),
    /// Native HLS with these GQL settings (only acted on in full builds).
    Native(GqlSettings),
}

/// The Twitch features one client may use: remote flags ∧ ¬kill-switches ∧
/// build flavor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitchFeatures {
    /// Player to use.
    pub player: PlayerChoice,
    /// VOD chapters from GQL `moments` (TWI-008).
    pub chapters_gql: bool,
    /// Online VOD chat replay through GQL.
    pub chat_replay: bool,
    /// Archive module (downloads, recording); full builds only.
    pub archive: bool,
}

impl TwitchFeatures {
    /// Resolves the features of this build ([`Flavor::CURRENT`]).
    pub fn resolve(config: &RemoteConfig, ctx: &ClientContext) -> Self {
        Self::resolve_for(config, ctx, Flavor::CURRENT)
    }

    /// Resolves the features for a given flavor (tests and diagnostics; a
    /// store build never acts on a `Native` choice).
    pub fn resolve_for(config: &RemoteConfig, ctx: &ClientContext, flavor: Flavor) -> Self {
        if flavor == Flavor::Store {
            return Self {
                player: PlayerChoice::Embed(EmbedReason::StoreBuild),
                chapters_gql: false,
                chat_replay: false,
                archive: false,
            };
        }
        let twitch = config.twitch_flags(ctx);
        let player = if let Some(ks) = config.kill_switch(flags::TWITCH_GQL_PLAYBACK, ctx) {
            PlayerChoice::Embed(EmbedReason::Killed {
                reason: ks.reason.clone(),
            })
        } else if !twitch.gql_playback {
            PlayerChoice::Embed(EmbedReason::GqlPlaybackOff)
        } else if twitch.player == PlayerMode::Embed {
            PlayerChoice::Embed(EmbedReason::PlayerEmbed)
        } else {
            match (
                twitch.gql_client_id,
                twitch
                    .persisted_queries
                    .get(PLAYBACK_ACCESS_TOKEN_OPERATION),
            ) {
                (Some(client_id), Some(hash)) if !client_id.is_empty() && !hash.is_empty() => {
                    PlayerChoice::Native(GqlSettings {
                        client_id,
                        playback_access_token_hash: hash.clone(),
                    })
                }
                _ => PlayerChoice::Embed(EmbedReason::MissingQueryHash),
            }
        };
        Self {
            player,
            chapters_gql: twitch.chapters_gql,
            chat_replay: twitch.chat_replay,
            archive: twitch.archive,
        }
    }
}

/// Play through the official embed (TWI-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedPlayback {
    /// What to play.
    pub target: PlaybackTarget,
    /// Why not native.
    pub reason: EmbedReason,
}

/// Native HLS playback, ready for the player (full builds only).
#[cfg(pa_twitch_native)]
#[derive(Debug, Clone, PartialEq)]
pub struct NativePlayback {
    /// What is played.
    pub target: PlaybackTarget,
    /// Token the master URL was built from.
    pub access_token: PlaybackAccessToken,
    /// usher master playlist URL (contains the anonymous token).
    pub master_url: String,
    /// Parsed variants of the master playlist.
    pub playlist: MasterPlaylist,
}

/// The answer of [`TwitchPlayback::resolve`].
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackDecision {
    /// Use the embed player.
    Embed(EmbedPlayback),
    /// Use the native player with this master playlist.
    #[cfg(pa_twitch_native)]
    Native(NativePlayback),
}

impl PlaybackDecision {
    /// The embed decision, if it is one.
    pub fn as_embed(&self) -> Option<&EmbedPlayback> {
        match self {
            PlaybackDecision::Embed(e) => Some(e),
            #[cfg(pa_twitch_native)]
            PlaybackDecision::Native(_) => None,
        }
    }
}

/// Native playback failures. The codes match the monitoring names of
/// 06 §2.7 (`gql_hash_invalid`, `integrity_required`, `usher_403`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaybackError {
    /// The login or video id is malformed.
    #[error("invalid playback target: {0}")]
    InvalidTarget(&'static str),
    /// No response (DNS, TLS, timeout).
    #[error("network error: {0}")]
    Transport(String),
    /// GQL does not know the persisted-query hash: the config is stale.
    #[error("Twitch GQL rejected the persisted-query hash")]
    GqlHashInvalid,
    /// GQL wants a client-integrity token, which we do not produce.
    #[error("Twitch GQL requires a client-integrity token")]
    IntegrityRequired,
    /// Any other GQL failure.
    #[error("Twitch GQL error {status}: {message}")]
    Gql {
        /// HTTP status.
        status: u16,
        /// Error message (short).
        message: String,
    },
    /// No such channel or video (the token is `null`).
    #[error("channel or video not found")]
    NotFound,
    /// usher has no playlist: the channel is offline or the VOD is gone.
    #[error("stream is not available")]
    Unavailable,
    /// usher answered 403 (sub-only VOD, geo block). Not bypassed.
    #[error("Twitch restricts this content: {code}")]
    Restricted {
        /// usher `error_code`, e.g. `vod_manifest_restricted`.
        code: String,
    },
    /// Any other usher status.
    #[error("usher error {status}")]
    Usher {
        /// HTTP status.
        status: u16,
    },
    /// The response is not what we expect.
    #[error("unexpected Twitch response: {0}")]
    Decode(String),
}

impl PlaybackError {
    /// Stable code for logs and diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            PlaybackError::InvalidTarget(_) => "invalid_target",
            PlaybackError::Transport(_) => "network",
            PlaybackError::GqlHashInvalid => "gql_hash_invalid",
            PlaybackError::IntegrityRequired => "integrity_required",
            PlaybackError::Gql { .. } => "gql_error",
            PlaybackError::NotFound => "not_found",
            PlaybackError::Unavailable => "unavailable",
            PlaybackError::Restricted { .. } => "usher_403",
            PlaybackError::Usher { .. } => "usher_error",
            PlaybackError::Decode(_) => "decode",
        }
    }

    /// Whether the official embed may still play this target (it carries
    /// the user's web session, sub status and Twitch's own integrity check).
    pub fn falls_back_to_embed(&self) -> bool {
        matches!(
            self,
            PlaybackError::GqlHashInvalid
                | PlaybackError::IntegrityRequired
                | PlaybackError::Gql { .. }
                | PlaybackError::Restricted { .. }
                | PlaybackError::Usher { .. }
                | PlaybackError::Decode(_)
        )
    }
}

/// Twitch playback entry point.
pub struct TwitchPlayback {
    features: TwitchFeatures,
    #[cfg(pa_twitch_native)]
    native: native::NativeResolver,
}

impl TwitchPlayback {
    /// Playback for these features over `transport`. In store builds the
    /// transport is never used.
    pub fn new(features: TwitchFeatures, transport: Arc<dyn Transport>) -> Self {
        #[cfg(not(pa_twitch_native))]
        let _ = transport;
        Self {
            features,
            #[cfg(pa_twitch_native)]
            native: native::NativeResolver::new(transport),
        }
    }

    /// The resolved features.
    pub fn features(&self) -> &TwitchFeatures {
        &self.features
    }

    /// Overrides the GQL endpoint and the usher base URL (tests, mirrors).
    #[cfg(pa_twitch_native)]
    pub fn with_endpoints(mut self, gql_url: &str, usher_base: &str) -> Self {
        self.native.gql_url = gql_url.trim_end_matches('/').to_owned();
        self.native.usher_base = usher_base.trim_end_matches('/').to_owned();
        self
    }

    /// How to play `target`. Embed decisions are made without any request.
    pub async fn resolve(
        &self,
        target: &PlaybackTarget,
        codecs: DeviceCodecs,
    ) -> Result<PlaybackDecision, PlaybackError> {
        let embed = |reason| {
            PlaybackDecision::Embed(EmbedPlayback {
                target: target.clone(),
                reason,
            })
        };
        match &self.features.player {
            PlayerChoice::Embed(reason) => Ok(embed(reason.clone())),
            #[cfg(pa_twitch_native)]
            PlayerChoice::Native(settings) => self
                .native
                .resolve(settings, target, codecs)
                .await
                .map(PlaybackDecision::Native),
            #[cfg(not(pa_twitch_native))]
            PlayerChoice::Native(_) => {
                let _ = codecs;
                Ok(embed(EmbedReason::StoreBuild))
            }
        }
    }

    /// Like [`Self::resolve`], but a native failure the embed may survive
    /// becomes an embed decision with [`EmbedReason::NativeFailed`].
    pub async fn resolve_or_embed(
        &self,
        target: &PlaybackTarget,
        codecs: DeviceCodecs,
    ) -> Result<PlaybackDecision, PlaybackError> {
        match self.resolve(target, codecs).await {
            Err(e) if e.falls_back_to_embed() => Ok(PlaybackDecision::Embed(EmbedPlayback {
                target: target.clone(),
                reason: EmbedReason::NativeFailed { code: e.code() },
            })),
            other => other,
        }
    }
}

#[cfg(pa_twitch_native)]
mod native {
    use std::sync::Arc;

    use super::{DeviceCodecs, GqlSettings, NativePlayback, PlaybackError, PlaybackTarget};
    use crate::transport::Transport;

    pub(super) struct NativeResolver {
        pub(super) transport: Arc<dyn Transport>,
        pub(super) gql_url: String,
        pub(super) usher_base: String,
    }

    impl NativeResolver {
        pub(super) fn new(transport: Arc<dyn Transport>) -> Self {
            Self {
                transport,
                gql_url: super::gql::GQL_URL.to_owned(),
                usher_base: super::usher::USHER_BASE.to_owned(),
            }
        }

        pub(super) async fn resolve(
            &self,
            settings: &GqlSettings,
            target: &PlaybackTarget,
            codecs: DeviceCodecs,
        ) -> Result<NativePlayback, PlaybackError> {
            let token =
                super::gql::fetch_token(&*self.transport, &self.gql_url, settings, target).await?;
            let master_url = super::usher::usher_master_url(
                &self.usher_base,
                target,
                &token,
                codecs,
                super::usher::random_nonce(),
            );
            let text = super::usher::fetch_master(&*self.transport, &master_url).await?;
            let playlist = super::hls::MasterPlaylist::parse(&text)?;
            Ok(NativePlayback {
                target: target.clone(),
                access_token: token,
                master_url,
                playlist,
            })
        }
    }
}
