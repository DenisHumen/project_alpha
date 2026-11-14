// SPDX-License-Identifier: MPL-2.0
//! `PlaybackAccessToken` over Twitch GQL (06 §2.1 step 1), full builds only.

use serde::Deserialize;
use serde_json::json;

use super::{GqlSettings, PLAYBACK_ACCESS_TOKEN_OPERATION, PlaybackError, PlaybackTarget};
use crate::transport::{HttpRequest, Method, Transport};

/// Default GQL endpoint.
pub(super) const GQL_URL: &str = "https://gql.twitch.tv/gql";

/// Signed playback token: `value` is a JSON document with the viewer's
/// rights, `signature` its signature. Both go to usher unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackAccessToken {
    /// Token JSON as a string.
    pub value: String,
    /// Signature of `value`.
    pub signature: String,
}

/// Request body. `playerType`/`platform` are the regular web-site player
/// values: never chosen to change what ads Twitch serves (06 §2.4).
pub(super) fn request_body(settings: &GqlSettings, target: &PlaybackTarget) -> serde_json::Value {
    let (is_live, login, is_vod, vod_id) = match target {
        PlaybackTarget::Live { login } => (true, login.as_str(), false, ""),
        PlaybackTarget::Vod { video_id } => (false, "", true, video_id.as_str()),
    };
    json!({
        "operationName": PLAYBACK_ACCESS_TOKEN_OPERATION,
        "variables": {
            "isLive": is_live,
            "login": login,
            "isVod": is_vod,
            "vodID": vod_id,
            "playerType": "site",
            "platform": "web",
        },
        "extensions": {
            "persistedQuery": {
                "version": 1,
                "sha256Hash": settings.playback_access_token_hash,
            },
        },
    })
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<GqlError>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    #[serde(default)]
    stream_playback_access_token: Option<RawToken>,
    #[serde(default)]
    video_playback_access_token: Option<RawToken>,
}

#[derive(Deserialize)]
struct RawToken {
    value: String,
    signature: String,
}

#[derive(Deserialize)]
struct GqlError {
    #[serde(default)]
    message: String,
}

/// Sends the request and maps the answer.
pub(super) async fn fetch_token(
    transport: &dyn Transport,
    url: &str,
    settings: &GqlSettings,
    target: &PlaybackTarget,
) -> Result<PlaybackAccessToken, PlaybackError> {
    let body = serde_json::to_vec(&request_body(settings, target))
        .map_err(|e| PlaybackError::Decode(e.to_string()))?;
    let request = HttpRequest {
        method: Method::Post,
        url: url.to_owned(),
        headers: vec![
            ("Client-ID".to_owned(), settings.client_id.clone()),
            ("Content-Type".to_owned(), "application/json".to_owned()),
        ],
        body: Some(body),
    };
    let response = transport
        .send(request)
        .await
        .map_err(|e| PlaybackError::Transport(e.0))?;
    parse_response(response.status, &response.body, target)
}

pub(super) fn parse_response(
    status: u16,
    body: &[u8],
    target: &PlaybackTarget,
) -> Result<PlaybackAccessToken, PlaybackError> {
    let parsed: Response = serde_json::from_slice(body).map_err(|e| {
        if (200..300).contains(&status) {
            PlaybackError::Decode(e.to_string())
        } else {
            PlaybackError::Gql {
                status,
                message: String::from_utf8_lossy(&body[..body.len().min(200)]).into_owned(),
            }
        }
    })?;
    if let Some(error) = parsed.errors.first() {
        let lower = error.message.to_ascii_lowercase();
        return Err(if lower.contains("persistedquerynotfound") {
            PlaybackError::GqlHashInvalid
        } else if lower.contains("integrity") {
            PlaybackError::IntegrityRequired
        } else {
            PlaybackError::Gql {
                status,
                message: error.message.clone(),
            }
        });
    }
    if !(200..300).contains(&status) {
        return Err(PlaybackError::Gql {
            status,
            message: parsed.message.unwrap_or_default(),
        });
    }
    let data = parsed
        .data
        .ok_or_else(|| PlaybackError::Decode("no data".into()))?;
    let token = match target {
        PlaybackTarget::Live { .. } => data.stream_playback_access_token,
        PlaybackTarget::Vod { .. } => data.video_playback_access_token,
    };
    let token = token.ok_or(PlaybackError::NotFound)?;
    Ok(PlaybackAccessToken {
        value: token.value,
        signature: token.signature,
    })
}
