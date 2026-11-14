// SPDX-License-Identifier: MPL-2.0
//! Normalised chat model shared by both sources (and by VOD replay later).
//!
//! Emotes and badges are raw ids here; turning them into images is TWI-011.

use std::collections::BTreeMap;
use std::time::Duration;

/// Channel an event belongs to. IRC knows the login from `#channel` and the
/// id from the `room-id` tag; EventSub always sends both.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ChannelRef {
    /// Broadcaster user id (empty if the server did not send it).
    pub id: String,
    /// Broadcaster login, lower case.
    pub login: String,
}

/// A chat badge as raw ids: `set_id` + `version` (`subscriber` / `12`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Badge {
    /// Badge set, e.g. `subscriber`, `moderator`, `bits`.
    pub set_id: String,
    /// Version inside the set (`id` in EventSub).
    pub version: String,
    /// Extra info, e.g. exact subscription months (`badge-info` in IRC).
    pub info: Option<String>,
}

/// Author of a message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatUser {
    /// User id (empty for anonymous gifters).
    pub id: String,
    /// Login, lower case.
    pub login: String,
    /// Display name (may differ from login in case or script).
    pub display_name: String,
    /// Name colour `#RRGGBB`, if the user picked one.
    pub color: Option<String>,
    /// Badges in display order.
    pub badges: Vec<Badge>,
}

/// An emote occurrence in [`ChatMessage::text`].
///
/// `start` and `end` are **character** (Unicode scalar value) offsets,
/// half-open: `text.chars().skip(start).take(end - start)` is the emote
/// code. IRC sends inclusive code-point ranges; EventSub sends fragments; both
/// are converted to this form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmoteRange {
    /// Twitch emote id (`emotesv2_…` or numeric).
    pub id: String,
    /// First character.
    pub start: usize,
    /// One past the last character.
    pub end: usize,
}

/// The message this one replies to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplyInfo {
    /// Id of the direct parent.
    pub parent_message_id: String,
    /// Parent author id.
    pub parent_user_id: String,
    /// Parent author login.
    pub parent_user_login: String,
    /// Parent author display name.
    pub parent_display_name: String,
    /// Parent text (as Twitch quotes it).
    pub parent_body: String,
    /// Id of the top message of the thread.
    pub thread_message_id: Option<String>,
    /// Login of the thread starter.
    pub thread_user_login: Option<String>,
}

/// Kind of a system notice (subscriptions, raids, announcements, …).
///
/// IRC `USERNOTICE` `msg-id` and EventSub `notice_type` use different
/// spellings (`subgift` vs `sub_gift`); both map here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoticeKind {
    /// New subscription.
    Sub,
    /// Re-subscription.
    Resub,
    /// A gifted subscription to one user.
    SubGift,
    /// A batch of gifted subscriptions to the community.
    CommunitySubGift,
    /// Gifted sub continued as paid.
    GiftPaidUpgrade,
    /// Prime sub continued as paid.
    PrimePaidUpgrade,
    /// Incoming raid (TWI-014 shows a toast, no auto-switch).
    Raid {
        /// Raiders, if known.
        viewer_count: Option<u32>,
    },
    /// Raid cancelled.
    Unraid,
    /// Highlighted announcement by a moderator.
    Announcement {
        /// Colour name (`PRIMARY`, `BLUE`, …).
        color: Option<String>,
    },
    /// New bits badge tier.
    BitsBadgeTier,
    /// Charity donation.
    CharityDonation,
    /// Anything else, with the raw source name.
    Other(String),
}

/// System part of a message: what Twitch announces (e.g. "X subscribed for
/// 12 months"), optionally followed by the user's own text in
/// [`ChatMessage::text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemNotice {
    /// Normalised kind.
    pub kind: NoticeKind,
    /// Ready-made text from Twitch (`system-msg` / `system_message`).
    pub system_text: String,
    /// Source-specific parameters as strings (IRC `msg-param-*` without the
    /// prefix; EventSub: fields of the per-type object). Keys differ between
    /// sources; use the typed fields of [`NoticeKind`] where they exist.
    pub params: BTreeMap<String, String>,
}

/// One chat line, normalised from IRC `PRIVMSG`/`USERNOTICE` or EventSub
/// `channel.chat.message`/`channel.chat.notification`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatMessage {
    /// Message id (used by deletes and replies).
    pub id: String,
    /// Server timestamp, Unix milliseconds.
    pub ts_ms: i64,
    /// Channel.
    pub channel: ChannelRef,
    /// Author (for system notices: the subject, possibly anonymous).
    pub user: ChatUser,
    /// Text as typed (for replies Twitch keeps the leading `@login `).
    pub text: String,
    /// Emotes in `text`.
    pub emotes: Vec<EmoteRange>,
    /// `/me` message.
    pub is_action: bool,
    /// Bits cheered with this message.
    pub bits: Option<u32>,
    /// Reply / thread.
    pub reply: Option<ReplyInfo>,
    /// System notice (sub, raid, announcement, …); `None` for plain chat.
    pub notice: Option<SystemNotice>,
    /// First message of this user in the channel.
    pub is_first_message: bool,
    /// Highlighted with channel points.
    pub is_highlighted: bool,
    /// Channel-points reward that sent it.
    pub reward_id: Option<String>,
    /// Shared chat: the channel the message was originally posted in, when
    /// different from `channel`.
    pub source_channel_id: Option<String>,
}

/// What happened to a user's messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearAction {
    /// Permanent ban.
    Ban,
    /// Timeout for this long.
    Timeout(Duration),
    /// Messages cleared, reason unknown (EventSub
    /// `channel.chat.clear_user_messages` does not say).
    Cleared,
}

/// Chat room modes. IRC `ROOMSTATE` may carry only the changed fields, so
/// every field is optional; `None` means "unchanged".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoomModes {
    /// Emote-only mode.
    pub emote_only: Option<bool>,
    /// Followers-only: `Some(None)` off, `Some(Some(d))` on with the minimum
    /// follow age `d` (zero = any follower).
    pub followers_only: Option<Option<Duration>>,
    /// Unique-chat (r9k) mode.
    pub unique_chat: Option<bool>,
    /// Slow mode: `Some(None)` off, `Some(Some(d))` one message per `d`.
    pub slow: Option<Option<Duration>>,
    /// Subscribers-only mode.
    pub subscribers_only: Option<bool>,
}

impl RoomModes {
    /// Applies a (partial) update on top of the known state.
    pub fn merge(&mut self, update: &RoomModes) {
        if update.emote_only.is_some() {
            self.emote_only = update.emote_only;
        }
        if update.followers_only.is_some() {
            self.followers_only = update.followers_only;
        }
        if update.unique_chat.is_some() {
            self.unique_chat = update.unique_chat;
        }
        if update.slow.is_some() {
            self.slow = update.slow;
        }
        if update.subscribers_only.is_some() {
            self.subscribers_only = update.subscribers_only;
        }
    }
}

/// Connection state reported in the event stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStatus {
    /// Socket is open and the session is ready (IRC welcome / EventSub
    /// subscriptions created).
    Connected,
    /// Joined a channel (IRC).
    Joined(ChannelRef),
    /// Connection lost; a reconnect follows after `delay`.
    Reconnecting {
        /// 1-based attempt number since the last good session.
        attempt: u32,
        /// Backoff before the attempt (zero when Twitch asked to move).
        delay: Duration,
        /// Why the old connection ended.
        reason: String,
    },
    /// The source gave up (auth revoked, subscription revoked, closed).
    Stopped {
        /// Why.
        reason: String,
    },
}

/// Everything a [`crate::ChatSource`] emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// A chat line or system notice (boxed: the other events are small).
    Message(Box<ChatMessage>),
    /// One message deleted by a moderator (show `<message deleted>`).
    MessageDeleted {
        /// Channel.
        channel: ChannelRef,
        /// Deleted message id.
        message_id: String,
        /// Author login, if known.
        user_login: Option<String>,
    },
    /// All messages of a user removed: ban, timeout or plain clear.
    UserCleared {
        /// Channel.
        channel: ChannelRef,
        /// Target user id.
        user_id: String,
        /// Target login.
        user_login: String,
        /// Ban, timeout or unknown.
        action: ClearAction,
    },
    /// The whole chat cleared by a moderator.
    ChatCleared {
        /// Channel.
        channel: ChannelRef,
    },
    /// Room modes (partial for IRC, full for EventSub).
    RoomModes {
        /// Channel.
        channel: ChannelRef,
        /// The modes carried by this update.
        modes: RoomModes,
    },
    /// Server notice (IRC `NOTICE`), e.g. "This room is in slow mode".
    Notice {
        /// Channel login, if the notice is per channel.
        channel_login: Option<String>,
        /// Machine id (`msg-id` tag).
        notice_id: Option<String>,
        /// Text.
        text: String,
    },
    /// Connection state.
    Status(SourceStatus),
}
