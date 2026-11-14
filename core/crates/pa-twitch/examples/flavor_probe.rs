// SPDX-License-Identifier: MPL-2.0
//! Probe binary for the store/full artifact check (CORE-013).
//!
//! Links the Twitch playback entry point the way an app does, so a release
//! build of it contains exactly the Twitch code of its flavor:
//!
//! ```sh
//! cargo build --release -p pa-twitch --example flavor_probe                         # store
//! cargo build --release -p pa-twitch --example flavor_probe --features pa-flavor/full
//! tools/flavor/check-artifacts.sh store target/release/examples/flavor_probe
//! ```
//!
//! Run, it prints the flavor and the playback decision for a channel under
//! the built-in remote config (always the embed: no request to Twitch).

use std::sync::Arc;
use std::time::Duration;

use pa_remote_config::ClientContext;
use pa_twitch::ReqwestTransport;
use pa_twitch::playback::{
    DeviceCodecs, Flavor, PlaybackDecision, PlaybackTarget, TwitchFeatures, TwitchPlayback,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let login = std::env::args().nth(1).unwrap_or_else(|| "kinolune".into());
    let target = match PlaybackTarget::live(&login) {
        Ok(target) => target,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let ctx = ClientContext {
        app_version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
    };
    let features = TwitchFeatures::resolve(&pa_remote_config::builtin(), &ctx);
    let transport = Arc::new(ReqwestTransport::new(Duration::from_secs(10)).expect("transport"));
    let playback = TwitchPlayback::new(features, transport);
    let decision = playback
        .resolve_or_embed(&target, DeviceCodecs::H264_ONLY)
        .await;
    let answer = match &decision {
        Ok(PlaybackDecision::Embed(e)) => format!("embed ({:?})", e.reason),
        #[cfg(pa_twitch_native)]
        Ok(PlaybackDecision::Native(n)) => format!("native ({} variants)", n.playlist.variants.len()),
        Err(e) => format!("error {}", e.code()),
    };
    println!("flavor {}: {answer}", Flavor::CURRENT);
}
