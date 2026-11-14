// SPDX-License-Identifier: MPL-2.0
//! Helix writes used by live chat (TWI-010): `POST /chat/messages` and
//! `POST /eventsub/subscriptions` with the WebSocket transport.
//!
//! The socket side (EventSub WebSocket, anonymous IRC) lives in
//! `pa-twitch-chat`; this crate only knows the documented HTTP endpoints.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::client::{Envelope, HelixClient};
use crate::error::{Error, Result};
use crate::transport::Method;

/// Longest chat message Helix accepts (in characters).
pub const MAX_CHAT_MESSAGE_CHARS: usize = 500;

/// Body of `POST /chat/messages` (scope `user:write:chat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SendChatMessage {
    /// Channel to post in.
    pub broadcaster_id: String,
    /// The signed-in user (must match the token owner).
    pub sender_id: String,
    /// Text, 1–500 characters.
    pub message: String,
    /// Message id this one answers (reply thread).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_parent_message_id: Option<String>,
}

/// Why Twitch dropped a sent message (AutoMod, slow mode, banned, …).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChatDropReason {
    /// Machine code, e.g. `msg_duplicate`, `msg_rejected`.
    #[serde(default)]
    pub code: String,
    /// Human-readable explanation from Twitch.
    #[serde(default)]
    pub message: String,
}

/// Answer of `POST /chat/messages`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SentChatMessage {
    /// Id of the new message (empty when dropped).
    #[serde(default)]
    pub message_id: String,
    /// `false` when Twitch dropped the message; see `drop_reason`.
    pub is_sent: bool,
    /// Reason of the drop.
    #[serde(default)]
    pub drop_reason: Option<ChatDropReason>,
}

/// Body of `POST /eventsub/subscriptions` over a WebSocket session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSubSubscriptionRequest {
    /// Subscription type, e.g. `channel.chat.message`.
    pub kind: String,
    /// Subscription version, e.g. `1`.
    pub version: String,
    /// Condition object (`broadcaster_user_id`, `user_id`, …).
    pub condition: BTreeMap<String, String>,
    /// Session id from the `session_welcome` message.
    pub session_id: String,
}

#[derive(Serialize)]
struct SubscriptionBody<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    version: &'a str,
    condition: &'a BTreeMap<String, String>,
    transport: WsTransport<'a>,
}

#[derive(Serialize)]
struct WsTransport<'a> {
    method: &'static str,
    session_id: &'a str,
}

/// A created EventSub subscription.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EventSubSubscription {
    /// Subscription id.
    pub id: String,
    /// `enabled` once active.
    pub status: String,
    /// Subscription type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Subscription version.
    pub version: String,
    /// Cost against `max_total_cost` (0 for chat the user authorised).
    #[serde(default)]
    pub cost: u32,
}

impl HelixClient {
    /// `POST /chat/messages`: sends a chat message as the signed-in user.
    ///
    /// A message Twitch refuses (AutoMod, slow mode, …) is still `Ok` with
    /// `is_sent == false` and a [`ChatDropReason`].
    pub async fn send_chat_message(&self, message: &SendChatMessage) -> Result<SentChatMessage> {
        let chars = message.message.chars().count();
        if chars == 0 || chars > MAX_CHAT_MESSAGE_CHARS {
            return Err(Error::InvalidArgument(
                "chat message must be 1-500 characters",
            ));
        }
        let body = serde_json::to_vec(message).map_err(|e| Error::Decode(e.to_string()))?;
        let env: Envelope<Vec<SentChatMessage>> = self.post_json("chat/messages", body).await?;
        env.data
            .into_iter()
            .next()
            .ok_or_else(|| Error::Decode("chat/messages: empty data".to_owned()))
    }

    /// `POST /eventsub/subscriptions` with `transport.method = websocket`.
    /// Must be called within 10 seconds of the session welcome.
    pub async fn create_eventsub_subscription(
        &self,
        request: &EventSubSubscriptionRequest,
    ) -> Result<EventSubSubscription> {
        let body = SubscriptionBody {
            kind: &request.kind,
            version: &request.version,
            condition: &request.condition,
            transport: WsTransport {
                method: "websocket",
                session_id: &request.session_id,
            },
        };
        let body = serde_json::to_vec(&body).map_err(|e| Error::Decode(e.to_string()))?;
        let env: Envelope<Vec<EventSubSubscription>> =
            self.post_json("eventsub/subscriptions", body).await?;
        env.data
            .into_iter()
            .next()
            .ok_or_else(|| Error::Decode("eventsub/subscriptions: empty data".to_owned()))
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: Vec<u8>,
    ) -> Result<Envelope<T>> {
        let mut request = self.build_request(Method::Post, path, &Vec::new())?;
        request
            .headers
            .push(("Content-Type".to_owned(), "application/json".to_owned()));
        request.body = Some(body);
        let response = self
            .send(request, false)
            .await?
            .ok_or_else(|| Error::Decode("empty response".to_owned()))?;
        serde_json::from_slice(&response.body).map_err(|e| Error::Decode(format!("{path}: {e}")))
    }
}
