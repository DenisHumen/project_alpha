// SPDX-License-Identifier: MPL-2.0
//! Sans-IO EventSub WebSocket: frame parser and normalisation of the chat
//! subscription types into [`ChatEvent`]s (research 06 §1.2, §3.1).
//!
//! Frames are JSON objects `{ "metadata": {…}, "payload": {…} }` with
//! `metadata.message_type` one of `session_welcome`, `session_keepalive`,
//! `notification`, `session_reconnect` or `revocation`.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::model::{
    Badge, ChannelRef, ChatEvent, ChatMessage, ChatUser, ClearAction, EmoteRange, NoticeKind,
    ReplyInfo, RoomModes, SystemNotice,
};

/// `channel.chat.message`: chat lines.
pub const SUB_CHAT_MESSAGE: &str = "channel.chat.message";
/// `channel.chat.notification`: subs, raids, announcements, …
pub const SUB_CHAT_NOTIFICATION: &str = "channel.chat.notification";
/// `channel.chat.message_delete`: one message removed.
pub const SUB_CHAT_MESSAGE_DELETE: &str = "channel.chat.message_delete";
/// `channel.chat.clear_user_messages`: ban or timeout.
pub const SUB_CHAT_CLEAR_USER: &str = "channel.chat.clear_user_messages";
/// `channel.chat.clear`: whole chat cleared.
pub const SUB_CHAT_CLEAR: &str = "channel.chat.clear";
/// `channel.chat_settings.update`: room modes.
pub const SUB_CHAT_SETTINGS: &str = "channel.chat_settings.update";

/// Subscription types the EventSub source creates per channel (all version
/// `1`, condition `broadcaster_user_id` + `user_id`, scope `user:read:chat`).
pub const CHAT_SUBSCRIPTIONS: &[&str] = &[
    SUB_CHAT_MESSAGE,
    SUB_CHAT_NOTIFICATION,
    SUB_CHAT_MESSAGE_DELETE,
    SUB_CHAT_CLEAR_USER,
    SUB_CHAT_CLEAR,
    SUB_CHAT_SETTINGS,
];

/// A decoded EventSub WebSocket frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventSubFrame {
    /// First frame of a session; subscriptions must be created within 10 s.
    Welcome {
        /// Session id for `transport.session_id`.
        session_id: String,
        /// Twitch closes the session if no frame arrives within this time.
        keepalive_timeout: Option<Duration>,
    },
    /// Nothing happened, the connection is alive.
    Keepalive,
    /// Twitch asks to move to `reconnect_url`; subscriptions carry over.
    Reconnect {
        /// URL of the new session.
        reconnect_url: String,
    },
    /// A subscribed event.
    Notification {
        /// Frame id, for de-duplication (Twitch may deliver twice).
        message_id: String,
        /// Subscription type.
        subscription_type: String,
        /// The normalised event; `None` for an unknown type.
        event: Option<ChatEvent>,
    },
    /// A subscription was revoked (token revoked, user banned, …).
    Revocation {
        /// Subscription type.
        subscription_type: String,
        /// Reason (`authorization_revoked`, `user_removed`, …).
        status: String,
    },
    /// Unknown `message_type`.
    Unknown(String),
}

/// Why a frame could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("bad EventSub frame: {0}")]
pub struct FrameError(pub String);

#[derive(Deserialize)]
struct Frame {
    metadata: Metadata,
    #[serde(default)]
    payload: Value,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    message_id: String,
    message_type: String,
    #[serde(default)]
    message_timestamp: String,
    #[serde(default)]
    subscription_type: Option<String>,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    #[serde(default)]
    keepalive_timeout_seconds: Option<u64>,
    #[serde(default)]
    reconnect_url: Option<String>,
}

#[derive(Deserialize)]
struct SubscriptionInfo {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    status: String,
}

/// Decodes one WebSocket text frame.
pub fn parse_frame(text: &str) -> Result<EventSubFrame, FrameError> {
    let frame: Frame = serde_json::from_str(text).map_err(|e| FrameError(e.to_string()))?;
    let payload = frame.payload;
    match frame.metadata.message_type.as_str() {
        "session_welcome" => {
            let session = session(&payload)?;
            Ok(EventSubFrame::Welcome {
                session_id: session.id,
                keepalive_timeout: session.keepalive_timeout_seconds.map(Duration::from_secs),
            })
        }
        "session_keepalive" => Ok(EventSubFrame::Keepalive),
        "session_reconnect" => {
            let session = session(&payload)?;
            Ok(EventSubFrame::Reconnect {
                reconnect_url: session
                    .reconnect_url
                    .ok_or_else(|| FrameError("session_reconnect without URL".to_owned()))?,
            })
        }
        "notification" => {
            let subscription_type = frame.metadata.subscription_type.clone().unwrap_or_default();
            let ts_ms = parse_rfc3339_ms(&frame.metadata.message_timestamp).unwrap_or_default();
            let event = payload.get("event").cloned().unwrap_or(Value::Null);
            Ok(EventSubFrame::Notification {
                message_id: frame.metadata.message_id,
                event: normalise(&subscription_type, event, ts_ms)?,
                subscription_type,
            })
        }
        "revocation" => {
            let sub: SubscriptionInfo = payload
                .get("subscription")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|e| FrameError(e.to_string()))?
                .ok_or_else(|| FrameError("revocation without subscription".to_owned()))?;
            Ok(EventSubFrame::Revocation {
                subscription_type: sub.kind,
                status: sub.status,
            })
        }
        other => Ok(EventSubFrame::Unknown(other.to_owned())),
    }
}

fn session(payload: &Value) -> Result<Session, FrameError> {
    payload
        .get("session")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| FrameError(e.to_string()))?
        .ok_or_else(|| FrameError("frame without session".to_owned()))
}

fn decode<T: serde::de::DeserializeOwned>(kind: &str, event: Value) -> Result<T, FrameError> {
    serde_json::from_value(event).map_err(|e| FrameError(format!("{kind}: {e}")))
}

/// Normalises the `event` object of a notification. `ts_ms` is the frame
/// timestamp (chat events carry none of their own).
pub fn normalise(kind: &str, event: Value, ts_ms: i64) -> Result<Option<ChatEvent>, FrameError> {
    Ok(Some(match kind {
        SUB_CHAT_MESSAGE => ChatEvent::Message(Box::new(
            decode::<ChatMessageEvent>(kind, event)?.into_message(ts_ms),
        )),
        SUB_CHAT_NOTIFICATION => ChatEvent::Message(Box::new(
            decode::<NotificationEvent>(kind, event)?.into_message(ts_ms),
        )),
        SUB_CHAT_MESSAGE_DELETE => {
            let e: MessageDeleteEvent = decode(kind, event)?;
            ChatEvent::MessageDeleted {
                channel: e.broadcaster.channel(),
                message_id: e.message_id,
                user_login: Some(e.target_user_login).filter(|l| !l.is_empty()),
            }
        }
        SUB_CHAT_CLEAR_USER => {
            let e: ClearUserEvent = decode(kind, event)?;
            ChatEvent::UserCleared {
                channel: e.broadcaster.channel(),
                user_id: e.target_user_id,
                user_login: e.target_user_login,
                action: ClearAction::Cleared,
            }
        }
        SUB_CHAT_CLEAR => {
            let e: BroadcasterOnly = decode(kind, event)?;
            ChatEvent::ChatCleared {
                channel: e.broadcaster.channel(),
            }
        }
        SUB_CHAT_SETTINGS => {
            let e: SettingsEvent = decode(kind, event)?;
            ChatEvent::RoomModes {
                channel: e.broadcaster.channel(),
                modes: RoomModes {
                    emote_only: Some(e.emote_mode),
                    followers_only: Some(e.follower_mode.then(|| {
                        Duration::from_secs(e.follower_mode_duration_minutes.unwrap_or(0) * 60)
                    })),
                    unique_chat: Some(e.unique_chat_mode),
                    slow: Some(
                        e.slow_mode.then(|| {
                            Duration::from_secs(e.slow_mode_wait_time_seconds.unwrap_or(0))
                        }),
                    ),
                    subscribers_only: Some(e.subscriber_mode),
                },
            }
        }
        _ => return Ok(None),
    }))
}

// ----- event payloads -------------------------------------------------------

#[derive(Deserialize)]
struct Broadcaster {
    broadcaster_user_id: String,
    broadcaster_user_login: String,
}

impl Broadcaster {
    fn channel(&self) -> ChannelRef {
        ChannelRef {
            id: self.broadcaster_user_id.clone(),
            login: self.broadcaster_user_login.to_lowercase(),
        }
    }
}

#[derive(Deserialize)]
struct BroadcasterOnly {
    #[serde(flatten)]
    broadcaster: Broadcaster,
}

#[derive(Deserialize)]
struct EsBadge {
    set_id: String,
    id: String,
    #[serde(default)]
    info: Option<String>,
}

#[derive(Deserialize, Default)]
struct EsMessage {
    #[serde(default)]
    text: String,
    #[serde(default)]
    fragments: Vec<Fragment>,
}

#[derive(Deserialize)]
struct Fragment {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    emote: Option<FragmentEmote>,
}

#[derive(Deserialize)]
struct FragmentEmote {
    id: String,
}

#[derive(Deserialize)]
struct Cheer {
    bits: u32,
}

#[derive(Deserialize)]
struct EsReply {
    parent_message_id: String,
    #[serde(default)]
    parent_message_body: String,
    #[serde(default)]
    parent_user_id: String,
    #[serde(default)]
    parent_user_name: String,
    #[serde(default)]
    parent_user_login: String,
    #[serde(default)]
    thread_message_id: Option<String>,
    #[serde(default)]
    thread_user_login: Option<String>,
}

#[derive(Deserialize)]
struct Chatter {
    #[serde(default)]
    chatter_user_id: Option<String>,
    #[serde(default)]
    chatter_user_login: Option<String>,
    #[serde(default)]
    chatter_user_name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    badges: Option<Vec<EsBadge>>,
}

impl Chatter {
    fn user(self) -> ChatUser {
        let login = self.chatter_user_login.unwrap_or_default().to_lowercase();
        ChatUser {
            id: self.chatter_user_id.unwrap_or_default(),
            display_name: self
                .chatter_user_name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| login.clone()),
            login,
            color: self.color.filter(|c| !c.is_empty()),
            badges: self
                .badges
                .unwrap_or_default()
                .into_iter()
                .map(|b| Badge {
                    set_id: b.set_id,
                    version: b.id,
                    info: b.info.filter(|i| !i.is_empty()),
                })
                .collect(),
        }
    }
}

#[derive(Deserialize)]
struct ChatMessageEvent {
    #[serde(flatten)]
    broadcaster: Broadcaster,
    #[serde(flatten)]
    chatter: Chatter,
    message_id: String,
    #[serde(default)]
    message: EsMessage,
    #[serde(default)]
    message_type: String,
    #[serde(default)]
    cheer: Option<Cheer>,
    #[serde(default)]
    reply: Option<EsReply>,
    #[serde(default)]
    channel_points_custom_reward_id: Option<String>,
    #[serde(default)]
    source_broadcaster_user_id: Option<String>,
}

/// Text and emote ranges from fragments. Twitch guarantees that the
/// fragments concatenate to `message.text`; if they do not, the text wins and
/// no emote ranges are reported.
fn text_and_emotes(message: EsMessage) -> (String, Vec<EmoteRange>) {
    let mut joined = String::new();
    let mut offset = 0usize;
    let mut emotes = Vec::new();
    for f in &message.fragments {
        let len = f.text.chars().count();
        if f.kind == "emote"
            && let Some(e) = &f.emote
        {
            emotes.push(EmoteRange {
                id: e.id.clone(),
                start: offset,
                end: offset + len,
            });
        }
        joined.push_str(&f.text);
        offset += len;
    }
    if message.fragments.is_empty() || joined == message.text {
        (message.text, emotes)
    } else {
        (message.text, Vec::new())
    }
}

fn reply(reply: Option<EsReply>) -> Option<ReplyInfo> {
    reply.map(|r| ReplyInfo {
        parent_message_id: r.parent_message_id,
        parent_user_id: r.parent_user_id,
        parent_user_login: r.parent_user_login,
        parent_display_name: r.parent_user_name,
        parent_body: r.parent_message_body,
        thread_message_id: r.thread_message_id.filter(|t| !t.is_empty()),
        thread_user_login: r.thread_user_login.filter(|t| !t.is_empty()),
    })
}

impl ChatMessageEvent {
    fn into_message(self, ts_ms: i64) -> ChatMessage {
        let channel = self.broadcaster.channel();
        let (text, emotes) = text_and_emotes(self.message);
        ChatMessage {
            id: self.message_id,
            ts_ms,
            user: self.chatter.user(),
            text,
            emotes,
            is_action: false,
            bits: self.cheer.map(|c| c.bits),
            reply: reply(self.reply),
            notice: None,
            is_first_message: self.message_type == "user_intro",
            is_highlighted: self.message_type == "channel_points_highlighted",
            reward_id: self
                .channel_points_custom_reward_id
                .filter(|r| !r.is_empty()),
            source_channel_id: self
                .source_broadcaster_user_id
                .filter(|s| !s.is_empty() && *s != channel.id),
            channel,
        }
    }
}

#[derive(Deserialize)]
struct NotificationEvent {
    #[serde(flatten)]
    broadcaster: Broadcaster,
    #[serde(flatten)]
    chatter: Chatter,
    message_id: String,
    #[serde(default)]
    system_message: String,
    #[serde(default)]
    message: EsMessage,
    notice_type: String,
    #[serde(default)]
    source_broadcaster_user_id: Option<String>,
    #[serde(flatten)]
    rest: BTreeMap<String, Value>,
}

impl NotificationEvent {
    fn into_message(mut self, ts_ms: i64) -> ChatMessage {
        let channel = self.broadcaster.channel();
        // Per-type details live in an object named after the notice type
        // (`raid`, `resub`, `announcement`, …); flatten it into strings.
        let details = self
            .rest
            .remove(&self.notice_type)
            .and_then(|v| match v {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default();
        let params: BTreeMap<String, String> = details
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) => Some((k, s)),
                Value::Number(n) => Some((k, n.to_string())),
                Value::Bool(b) => Some((k, b.to_string())),
                _ => None,
            })
            .collect();
        let kind = match self.notice_type.as_str() {
            "sub" => NoticeKind::Sub,
            "resub" => NoticeKind::Resub,
            "sub_gift" => NoticeKind::SubGift,
            "community_sub_gift" => NoticeKind::CommunitySubGift,
            "gift_paid_upgrade" => NoticeKind::GiftPaidUpgrade,
            "prime_paid_upgrade" => NoticeKind::PrimePaidUpgrade,
            "raid" => NoticeKind::Raid {
                viewer_count: params.get("viewer_count").and_then(|v| v.parse().ok()),
            },
            "unraid" => NoticeKind::Unraid,
            "announcement" => NoticeKind::Announcement {
                color: params.get("color").cloned(),
            },
            "bits_badge_tier" => NoticeKind::BitsBadgeTier,
            "charity_donation" => NoticeKind::CharityDonation,
            other => NoticeKind::Other(other.to_owned()),
        };
        let (text, emotes) = text_and_emotes(self.message);
        ChatMessage {
            id: self.message_id,
            ts_ms,
            user: self.chatter.user(),
            text,
            emotes,
            notice: Some(SystemNotice {
                kind,
                system_text: self.system_message,
                params,
            }),
            source_channel_id: self
                .source_broadcaster_user_id
                .filter(|s| !s.is_empty() && *s != channel.id),
            channel,
            ..ChatMessage::default()
        }
    }
}

#[derive(Deserialize)]
struct MessageDeleteEvent {
    #[serde(flatten)]
    broadcaster: Broadcaster,
    #[serde(default)]
    target_user_login: String,
    message_id: String,
}

#[derive(Deserialize)]
struct ClearUserEvent {
    #[serde(flatten)]
    broadcaster: Broadcaster,
    target_user_id: String,
    target_user_login: String,
}

#[derive(Deserialize)]
struct SettingsEvent {
    #[serde(flatten)]
    broadcaster: Broadcaster,
    emote_mode: bool,
    follower_mode: bool,
    #[serde(default)]
    follower_mode_duration_minutes: Option<u64>,
    slow_mode: bool,
    #[serde(default)]
    slow_mode_wait_time_seconds: Option<u64>,
    subscriber_mode: bool,
    unique_chat_mode: bool,
}

// ----- time -----------------------------------------------------------------

/// Parses an RFC 3339 UTC timestamp (`2026-11-14T18:11:47.492253549Z`, also
/// with a `+hh:mm` offset) into Unix milliseconds.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let mut rest = &s[19..];
    let mut millis = 0i64;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let ms: String = frac[..digits]
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        millis = ms.parse().ok()?;
        rest = &frac[digits..];
    }
    let offset_secs = match rest {
        "Z" | "z" => 0,
        tz if tz.len() == 6 && matches!(&tz[..1], "+" | "-") && &tz[3..4] == ":" => {
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            sign * (tz[1..3].parse::<i64>().ok()? * 3600 + tz[4..6].parse::<i64>().ok()? * 60)
        }
        _ => return None,
    };
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset_secs;
    Some(secs * 1000 + millis)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
