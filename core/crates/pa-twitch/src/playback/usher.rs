// SPDX-License-Identifier: MPL-2.0
//! usher master playlist (06 §2.1 step 2), full builds only.

use std::hash::{BuildHasher, Hasher};

use serde::Deserialize;

use super::{DeviceCodecs, PlaybackAccessToken, PlaybackError, PlaybackTarget};
use crate::transport::{HttpRequest, Method, Transport};
use crate::util::encode_query_component;

/// Default usher base URL.
pub(super) const USHER_BASE: &str = "https://usher.ttvnw.net";

/// Master playlist URL for `target`:
/// `{base}/api/channel/hls/{login}.m3u8?…` (live) or
/// `{base}/vod/{id}.m3u8?…` (VOD). `nonce` fills the `p` cache-buster.
pub fn usher_master_url(
    base: &str,
    target: &PlaybackTarget,
    token: &PlaybackAccessToken,
    codecs: DeviceCodecs,
    nonce: u32,
) -> String {
    let path = match target {
        PlaybackTarget::Live { login } => format!("api/channel/hls/{login}.m3u8"),
        PlaybackTarget::Vod { video_id } => format!("vod/{video_id}.m3u8"),
    };
    let query = [
        ("allow_source", "true".to_owned()),
        ("allow_audio_only", "true".to_owned()),
        ("playlist_include_framerate", "true".to_owned()),
        ("supported_codecs", codecs.supported_codecs()),
        ("sig", token.signature.clone()),
        ("token", token.value.clone()),
        ("p", nonce.to_string()),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", encode_query_component(v)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{}/{path}?{query}", base.trim_end_matches('/'))
}

/// Random `p` value (0..10_000_000), like the web player.
pub(super) fn random_nonce() -> u32 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    (hasher.finish() % 10_000_000) as u32
}

#[derive(Deserialize)]
struct UsherError {
    #[serde(default)]
    error_code: String,
}

/// Downloads the master playlist text. The body is returned as is.
pub(super) async fn fetch_master(
    transport: &dyn Transport,
    url: &str,
) -> Result<String, PlaybackError> {
    let response = transport
        .send(HttpRequest {
            method: Method::Get,
            url: url.to_owned(),
            headers: vec![(
                "Accept".to_owned(),
                "application/vnd.apple.mpegurl".to_owned(),
            )],
            body: None,
        })
        .await
        .map_err(|e| PlaybackError::Transport(e.0))?;
    let code = || {
        serde_json::from_slice::<Vec<UsherError>>(&response.body)
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|e| e.error_code)
            .filter(|c| !c.is_empty())
    };
    match response.status {
        200..=299 => String::from_utf8(response.body.clone())
            .map_err(|_| PlaybackError::Decode("playlist is not UTF-8".into())),
        403 => Err(PlaybackError::Restricted {
            code: code().unwrap_or_else(|| "forbidden".into()),
        }),
        404 => Err(PlaybackError::Unavailable),
        status => Err(PlaybackError::Usher { status }),
    }
}
