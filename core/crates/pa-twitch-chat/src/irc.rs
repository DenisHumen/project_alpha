// SPDX-License-Identifier: MPL-2.0
//! Sans-IO Twitch IRC: IRCv3 line parser and normalisation into
//! [`ChatEvent`]s (research 06 §3.1).
//!
//! Twitch IRC over WebSocket sends one or more `\r\n`-terminated lines per
//! frame. With the `twitch.tv/tags` and `twitch.tv/commands` capabilities
//! the lines carry IRCv3 tags (`badges`, `emotes`, `color`, `reply-parent-*`,
//! `msg-id`, …) and the moderation commands `CLEARCHAT`, `CLEARMSG` and
//! `ROOMSTATE`.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::model::{
    Badge, ChannelRef, ChatEvent, ChatMessage, ChatUser, ClearAction, EmoteRange, NoticeKind,
    ReplyInfo, RoomModes, SourceStatus, SystemNotice,
};

/// One parsed IRC line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IrcMessage {
    /// IRCv3 tags, unescaped. A tag without `=` maps to an empty string.
    pub tags: BTreeMap<String, String>,
    /// Prefix without the leading `:` (`nick!user@host` or a server name).
    pub prefix: Option<String>,
    /// Command (`PRIVMSG`, `001`, …), upper case as sent.
    pub command: String,
    /// Middle parameters followed by the trailing one (without `:`).
    pub params: Vec<String>,
}

impl IrcMessage {
    /// Tag value, `None` when absent or empty.
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// Nick from the prefix (`nick!nick@nick.tmi.twitch.tv`).
    pub fn nick(&self) -> Option<&str> {
        let prefix = self.prefix.as_deref()?;
        let nick = prefix.split('!').next()?;
        (!nick.is_empty() && prefix.contains('!')).then_some(nick)
    }

    /// Parameter `i`.
    pub fn param(&self, i: usize) -> Option<&str> {
        self.params.get(i).map(String::as_str)
    }
}

/// Parses one IRC line (without `\r\n`). `None` for an empty or malformed
/// line.
pub fn parse_line(line: &str) -> Option<IrcMessage> {
    let mut rest = line.trim_end_matches(['\r', '\n']);
    let mut msg = IrcMessage::default();

    if let Some(after) = rest.strip_prefix('@') {
        let (tags, tail) = after.split_once(' ')?;
        for tag in tags.split(';').filter(|t| !t.is_empty()) {
            let (key, value) = tag.split_once('=').unwrap_or((tag, ""));
            msg.tags.insert(key.to_owned(), unescape_tag(value));
        }
        rest = tail.trim_start_matches(' ');
    }
    if let Some(after) = rest.strip_prefix(':') {
        let (prefix, tail) = after.split_once(' ')?;
        msg.prefix = Some(prefix.to_owned());
        rest = tail.trim_start_matches(' ');
    }
    let (command, mut rest) = match rest.split_once(' ') {
        Some((c, r)) => (c, r),
        None => (rest, ""),
    };
    if command.is_empty() {
        return None;
    }
    msg.command = command.to_owned();
    loop {
        rest = rest.trim_start_matches(' ');
        if rest.is_empty() {
            break;
        }
        if let Some(trailing) = rest.strip_prefix(':') {
            msg.params.push(trailing.to_owned());
            break;
        }
        match rest.split_once(' ') {
            Some((p, r)) => {
                msg.params.push(p.to_owned());
                rest = r;
            }
            None => {
                msg.params.push(rest.to_owned());
                break;
            }
        }
    }
    Some(msg)
}

/// Splits a WebSocket frame into lines and parses each.
pub fn parse_frame(frame: &str) -> Vec<IrcMessage> {
    frame
        .split("\r\n")
        .flat_map(|l| l.split('\n'))
        .filter(|l| !l.trim().is_empty())
        .filter_map(parse_line)
        .collect()
}

/// IRCv3 tag value unescaping (`\s`, `\:`, `\\`, `\r`, `\n`).
pub fn unescape_tag(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(':') => out.push(';'),
            Some('s') => out.push(' '),
            Some('\\') => out.push('\\'),
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// What the connection loop must do with a parsed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrcInput {
    /// Emit to the consumer.
    Event(ChatEvent),
    /// Answer `PONG :<payload>`.
    Ping(String),
    /// `RECONNECT`: Twitch restarts the server; reconnect now, no backoff.
    Reconnect,
    /// `001`: registration accepted.
    Welcome,
    /// Not interesting (`CAP`, `USERSTATE`, numerics, …).
    Ignore,
}

/// Interprets a parsed line. `own_nick` filters our own `JOIN` echoes.
pub fn interpret(msg: &IrcMessage, own_nick: &str) -> IrcInput {
    match msg.command.as_str() {
        "PING" => IrcInput::Ping(msg.param(0).unwrap_or("tmi.twitch.tv").to_owned()),
        "RECONNECT" => IrcInput::Reconnect,
        "001" => IrcInput::Welcome,
        "JOIN" if msg.nick().is_some_and(|n| n.eq_ignore_ascii_case(own_nick)) => {
            IrcInput::Event(ChatEvent::Status(SourceStatus::Joined(ChannelRef {
                id: String::new(),
                login: channel_login(msg.param(0)),
            })))
        }
        "PRIVMSG" => privmsg(msg).map_or(IrcInput::Ignore, |m| {
            IrcInput::Event(ChatEvent::Message(Box::new(m)))
        }),
        "USERNOTICE" => IrcInput::Event(ChatEvent::Message(Box::new(usernotice(msg)))),
        "CLEARCHAT" => IrcInput::Event(clearchat(msg)),
        "CLEARMSG" => IrcInput::Event(ChatEvent::MessageDeleted {
            channel: channel(msg),
            message_id: msg.tag("target-msg-id").unwrap_or_default().to_owned(),
            user_login: msg.tag("login").map(str::to_owned),
        }),
        "ROOMSTATE" => IrcInput::Event(ChatEvent::RoomModes {
            channel: channel(msg),
            modes: roomstate(msg),
        }),
        "NOTICE" => {
            let target = msg.param(0).unwrap_or("*");
            IrcInput::Event(ChatEvent::Notice {
                channel_login: target.strip_prefix('#').map(str::to_lowercase),
                notice_id: msg.tag("msg-id").map(str::to_owned),
                text: msg.param(1).unwrap_or_default().to_owned(),
            })
        }
        _ => IrcInput::Ignore,
    }
}

fn channel_login(param: Option<&str>) -> String {
    param
        .unwrap_or_default()
        .trim_start_matches('#')
        .to_lowercase()
}

fn channel(msg: &IrcMessage) -> ChannelRef {
    ChannelRef {
        id: msg.tag("room-id").unwrap_or_default().to_owned(),
        login: channel_login(msg.param(0)),
    }
}

fn user(msg: &IrcMessage, login: &str) -> ChatUser {
    let login = login.to_lowercase();
    ChatUser {
        id: msg.tag("user-id").unwrap_or_default().to_owned(),
        display_name: msg
            .tag("display-name")
            .map(|d| d.trim().to_owned())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| login.clone()),
        login,
        color: msg.tag("color").map(str::to_owned),
        badges: badges(msg.tag("badges"), msg.tag("badge-info")),
    }
}

/// Parses `badges` (`subscriber/12,premium/1`) with `badge-info`
/// (`subscriber/14`) as the info of the same set.
pub fn badges(badges: Option<&str>, badge_info: Option<&str>) -> Vec<Badge> {
    let info: BTreeMap<&str, &str> = badge_info
        .unwrap_or_default()
        .split(',')
        .filter_map(|b| b.split_once('/'))
        .collect();
    badges
        .unwrap_or_default()
        .split(',')
        .filter_map(|b| b.split_once('/'))
        .filter(|(set, _)| !set.is_empty())
        .map(|(set, version)| Badge {
            set_id: set.to_owned(),
            version: version.to_owned(),
            info: info.get(set).map(|i| (*i).to_owned()),
        })
        .collect()
}

/// Parses the `emotes` tag (`25:0-4,12-16/1902:6-10`, inclusive code-point
/// ranges) into half-open character ranges sorted by position. Ranges that
/// do not fit into a text of `text_chars` characters are dropped.
pub fn emotes(tag: Option<&str>, text_chars: usize) -> Vec<EmoteRange> {
    let mut out: Vec<EmoteRange> = tag
        .unwrap_or_default()
        .split('/')
        .filter_map(|e| e.split_once(':'))
        .flat_map(|(id, ranges)| {
            ranges.split(',').filter_map(move |r| {
                let (a, b) = r.split_once('-')?;
                let start: usize = a.parse().ok()?;
                let last: usize = b.parse().ok()?;
                (start <= last && last < text_chars).then(|| EmoteRange {
                    id: id.to_owned(),
                    start,
                    end: last + 1,
                })
            })
        })
        .collect();
    out.sort_by_key(|e| e.start);
    out
}

fn ts_ms(msg: &IrcMessage) -> i64 {
    msg.tag("tmi-sent-ts")
        .and_then(|t| t.parse().ok())
        .unwrap_or_default()
}

fn reply(msg: &IrcMessage) -> Option<ReplyInfo> {
    let parent = msg.tag("reply-parent-msg-id")?;
    Some(ReplyInfo {
        parent_message_id: parent.to_owned(),
        parent_user_id: msg
            .tag("reply-parent-user-id")
            .unwrap_or_default()
            .to_owned(),
        parent_user_login: msg
            .tag("reply-parent-user-login")
            .unwrap_or_default()
            .to_owned(),
        parent_display_name: msg
            .tag("reply-parent-display-name")
            .unwrap_or_default()
            .to_owned(),
        parent_body: msg
            .tag("reply-parent-msg-body")
            .unwrap_or_default()
            .to_owned(),
        thread_message_id: msg.tag("reply-thread-parent-msg-id").map(str::to_owned),
        thread_user_login: msg.tag("reply-thread-parent-user-login").map(str::to_owned),
    })
}

fn base_message(msg: &IrcMessage, login: &str, raw_text: &str) -> ChatMessage {
    let (text, is_action) = match raw_text
        .strip_prefix("\u{1}ACTION ")
        .map(|t| t.strip_suffix('\u{1}').unwrap_or(t))
    {
        Some(inner) => (inner.to_owned(), true),
        None => (raw_text.to_owned(), false),
    };
    let channel = channel(msg);
    let source_channel_id = msg
        .tag("source-room-id")
        .filter(|s| *s != channel.id)
        .map(str::to_owned);
    ChatMessage {
        id: msg.tag("id").unwrap_or_default().to_owned(),
        ts_ms: ts_ms(msg),
        user: user(msg, login),
        emotes: emotes(msg.tag("emotes"), text.chars().count()),
        text,
        is_action,
        bits: msg.tag("bits").and_then(|b| b.parse().ok()),
        reply: reply(msg),
        notice: None,
        is_first_message: msg.tag("first-msg") == Some("1"),
        is_highlighted: msg.tag("msg-id") == Some("highlighted-message"),
        reward_id: msg.tag("custom-reward-id").map(str::to_owned),
        source_channel_id,
        channel,
    }
}

fn privmsg(msg: &IrcMessage) -> Option<ChatMessage> {
    let login = msg.nick()?;
    Some(base_message(msg, login, msg.param(1).unwrap_or_default()))
}

fn usernotice(msg: &IrcMessage) -> ChatMessage {
    let login = msg.tag("login").unwrap_or_default();
    let mut out = base_message(msg, login, msg.param(1).unwrap_or_default());
    let params: BTreeMap<String, String> = msg
        .tags
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("msg-param-")
                .map(|k| (k.to_owned(), v.clone()))
        })
        .collect();
    let kind = match msg.tag("msg-id").unwrap_or_default() {
        "sub" => NoticeKind::Sub,
        "resub" => NoticeKind::Resub,
        "subgift" => NoticeKind::SubGift,
        "submysterygift" => NoticeKind::CommunitySubGift,
        "giftpaidupgrade" | "anongiftpaidupgrade" => NoticeKind::GiftPaidUpgrade,
        "primepaidupgrade" => NoticeKind::PrimePaidUpgrade,
        "raid" => NoticeKind::Raid {
            viewer_count: params.get("viewerCount").and_then(|v| v.parse().ok()),
        },
        "unraid" => NoticeKind::Unraid,
        "announcement" => NoticeKind::Announcement {
            color: params.get("color").cloned(),
        },
        "bitsbadgetier" => NoticeKind::BitsBadgeTier,
        "charitydonation" => NoticeKind::CharityDonation,
        other => NoticeKind::Other(other.to_owned()),
    };
    out.notice = Some(SystemNotice {
        kind,
        system_text: msg.tag("system-msg").unwrap_or_default().to_owned(),
        params,
    });
    out
}

fn clearchat(msg: &IrcMessage) -> ChatEvent {
    let channel = channel(msg);
    match msg.param(1) {
        Some(login) => ChatEvent::UserCleared {
            channel,
            user_id: msg.tag("target-user-id").unwrap_or_default().to_owned(),
            user_login: login.to_lowercase(),
            action: match msg.tag("ban-duration").and_then(|d| d.parse::<u64>().ok()) {
                Some(secs) => ClearAction::Timeout(Duration::from_secs(secs)),
                None => ClearAction::Ban,
            },
        },
        None => ChatEvent::ChatCleared { channel },
    }
}

fn flag(msg: &IrcMessage, name: &str) -> Option<bool> {
    msg.tag(name).map(|v| v != "0")
}

fn roomstate(msg: &IrcMessage) -> RoomModes {
    RoomModes {
        emote_only: flag(msg, "emote-only"),
        // -1 = off, 0 = all followers, N = followed for N minutes.
        followers_only: msg
            .tag("followers-only")
            .and_then(|v| v.parse::<i64>().ok())
            .map(|m| (m >= 0).then(|| Duration::from_secs(m.unsigned_abs() * 60))),
        unique_chat: flag(msg, "r9k"),
        slow: msg
            .tag("slow")
            .and_then(|v| v.parse::<u64>().ok())
            .map(|s| (s > 0).then(|| Duration::from_secs(s))),
        subscribers_only: flag(msg, "subs-only"),
    }
}
