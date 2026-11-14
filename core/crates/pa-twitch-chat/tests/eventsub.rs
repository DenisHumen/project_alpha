// SPDX-License-Identifier: MPL-2.0
//! EventSub WebSocket frame parsing and normalisation on recorded
//! (hand-written, made-up names) frames: `tests/fixtures/eventsub/`.

use std::time::Duration;

use pa_twitch_chat::eventsub::{EventSubFrame, parse_frame, parse_rfc3339_ms};
use pa_twitch_chat::{
    Badge, ChannelRef, ChatEvent, ChatMessage, ClearAction, NoticeKind, RoomModes,
};

macro_rules! fx {
    ($name:literal) => {
        include_str!(concat!("fixtures/eventsub/", $name, ".json"))
    };
}

fn channel() -> ChannelRef {
    ChannelRef {
        id: "100000001".to_owned(),
        login: "kinolune_demo".to_owned(),
    }
}

fn event(text: &str) -> ChatEvent {
    match parse_frame(text).unwrap() {
        EventSubFrame::Notification {
            event: Some(e), ..
        } => e,
        other => panic!("expected a notification, got {other:?}"),
    }
}

fn message(text: &str) -> ChatMessage {
    match event(text) {
        ChatEvent::Message(m) => m,
        other => panic!("expected a message, got {other:?}"),
    }
}

#[test]
fn session_frames() {
    assert_eq!(
        parse_frame(fx!("welcome")).unwrap(),
        EventSubFrame::Welcome {
            session_id: "AgoQHR3s6Mb4T8GFB1l3DlPfiRIGY2VsbC1h".to_owned(),
            keepalive_timeout: Some(Duration::from_secs(10)),
        }
    );
    assert_eq!(
        parse_frame(fx!("keepalive")).unwrap(),
        EventSubFrame::Keepalive
    );
    assert_eq!(
        parse_frame(fx!("session_reconnect")).unwrap(),
        EventSubFrame::Reconnect {
            reconnect_url: "wss://eventsub.example.invalid/ws?id=reconnect-0001".to_owned(),
        }
    );
    assert_eq!(
        parse_frame(fx!("revocation")).unwrap(),
        EventSubFrame::Revocation {
            subscription_type: "channel.chat.message".to_owned(),
            status: "authorization_revoked".to_owned(),
        }
    );
    assert!(parse_frame("not json").is_err());
    assert_eq!(
        parse_frame(r#"{"metadata":{"message_type":"brand_new"},"payload":{}}"#).unwrap(),
        EventSubFrame::Unknown("brand_new".to_owned())
    );
}

#[test]
fn chat_message_with_fragments_badges_and_reply() {
    let frame = parse_frame(fx!("chat_message")).unwrap();
    let EventSubFrame::Notification {
        message_id,
        subscription_type,
        ..
    } = &frame
    else {
        panic!();
    };
    assert_eq!(message_id, "c1a2b3c4-0001-4000-8000-000000000001");
    assert_eq!(subscription_type, "channel.chat.message");

    let m = message(fx!("chat_message"));
    assert_eq!(m.id, "e5f6a7b8-0003-4a5b-9c1d-000000000003");
    assert_eq!(m.ts_ms, 1_794_657_601_250);
    assert_eq!(m.channel, channel());
    assert_eq!(m.user.id, "200000002");
    assert_eq!(m.user.login, "quiet_heron");
    assert_eq!(m.user.display_name, "QuietHeron");
    assert_eq!(m.user.color.as_deref(), Some("#8A2BE2"));
    assert_eq!(
        m.user.badges,
        vec![
            Badge {
                set_id: "moderator".to_owned(),
                version: "1".to_owned(),
                info: None,
            },
            Badge {
                set_id: "subscriber".to_owned(),
                version: "12".to_owned(),
                info: Some("15".to_owned()),
            },
        ]
    );
    assert_eq!(m.text, "@Pixel_Otter héllo Kappa 🎉 Keepo");
    let codes: Vec<(String, String)> = m
        .emotes
        .iter()
        .map(|e| {
            (
                e.id.clone(),
                m.text.chars().skip(e.start).take(e.end - e.start).collect(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            ("25".to_owned(), "Kappa".to_owned()),
            (
                "emotesv2_0f1e2d3c4b5a69788796a5b4c3d2e1f0".to_owned(),
                "Keepo".to_owned()
            ),
        ]
    );
    let reply = m.reply.as_ref().unwrap();
    assert_eq!(
        reply.parent_message_id,
        "e5f6a7b8-0001-4a5b-9c1d-000000000001"
    );
    assert_eq!(reply.parent_user_login, "pixel_otter");
    assert_eq!(reply.parent_display_name, "Pixel_Otter");
    assert_eq!(reply.parent_body, "Kappa Keepo Kappa");
    assert_eq!(reply.thread_user_login.as_deref(), Some("pixel_otter"));
    assert_eq!(m.bits, None);
    assert_eq!(m.notice, None);
    assert_eq!(m.source_channel_id, None);
}

#[test]
fn cheer_highlight_and_shared_chat() {
    let m = message(fx!("chat_message_cheer"));
    assert_eq!(m.bits, Some(100));
    assert!(m.is_highlighted);
    assert_eq!(m.user.color, None, "empty colour");
    assert!(m.emotes.is_empty());
    assert_eq!(m.source_channel_id.as_deref(), Some("100000009"));
}

#[test]
fn notifications() {
    let resub = message(fx!("notification_resub"));
    let notice = resub.notice.as_ref().unwrap();
    assert_eq!(notice.kind, NoticeKind::Resub);
    assert_eq!(
        notice.system_text,
        "QuietHeron subscribed at Tier 1. They've subscribed for 15 months!"
    );
    assert_eq!(notice.params["cumulative_months"], "15");
    assert_eq!(notice.params["sub_tier"], "1000");
    assert_eq!(notice.params["is_gift"], "false");
    assert!(!notice.params.contains_key("streak_months"), "nulls skipped");
    assert_eq!(resub.text, "fifteen months already");
    assert_eq!(resub.user.badges[0].info.as_deref(), Some("15"));

    let raid = message(fx!("notification_raid"));
    assert_eq!(
        raid.notice.as_ref().unwrap().kind,
        NoticeKind::Raid {
            viewer_count: Some(42)
        }
    );
    assert_eq!(raid.notice.as_ref().unwrap().params["user_login"], "moss_lantern");
    assert_eq!(raid.text, "");

    let gift = message(fx!("notification_announcement_anonymous"));
    assert_eq!(gift.notice.as_ref().unwrap().kind, NoticeKind::SubGift);
    assert_eq!(gift.user.id, "", "anonymous gifter");
    assert_eq!(gift.user.login, "");
    assert_eq!(gift.user.color, None);
    assert!(gift.user.badges.is_empty());
    assert_eq!(
        gift.notice.as_ref().unwrap().params["recipient_user_login"],
        "pixel_otter"
    );
}

#[test]
fn moderation_events() {
    assert_eq!(
        event(fx!("message_delete")),
        ChatEvent::MessageDeleted {
            channel: channel(),
            message_id: "e5f6a7b8-0001-4a5b-9c1d-000000000001".to_owned(),
            user_login: Some("pixel_otter".to_owned()),
        }
    );
    assert_eq!(
        event(fx!("clear_user_messages")),
        ChatEvent::UserCleared {
            channel: channel(),
            user_id: "200000004".to_owned(),
            user_login: "spam_sparrow".to_owned(),
            action: ClearAction::Cleared,
        }
    );
    assert_eq!(
        event(fx!("chat_clear")),
        ChatEvent::ChatCleared { channel: channel() }
    );
    assert_eq!(
        event(fx!("chat_settings_update")),
        ChatEvent::RoomModes {
            channel: channel(),
            modes: RoomModes {
                emote_only: Some(true),
                followers_only: Some(Some(Duration::from_secs(600))),
                unique_chat: Some(true),
                slow: Some(None),
                subscribers_only: Some(false),
            },
        }
    );
}

#[test]
fn unknown_subscription_type_is_not_an_error() {
    let frame = r#"{"metadata":{"message_id":"x","message_type":"notification","message_timestamp":"2026-11-14T12:00:00Z","subscription_type":"channel.shiny_new"},"payload":{"event":{"anything":1}}}"#;
    assert_eq!(
        parse_frame(frame).unwrap(),
        EventSubFrame::Notification {
            message_id: "x".to_owned(),
            subscription_type: "channel.shiny_new".to_owned(),
            event: None,
        }
    );
}

#[test]
fn rfc3339_timestamps() {
    assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        parse_rfc3339_ms("2026-11-14T12:00:01.250000000Z"),
        Some(1_794_657_601_250)
    );
    assert_eq!(
        parse_rfc3339_ms("2026-11-14T14:00:01.25+02:00"),
        Some(1_794_657_601_250)
    );
    assert_eq!(parse_rfc3339_ms("2024-02-29T23:59:59.9Z"), Some(1_709_251_199_900));
    assert_eq!(parse_rfc3339_ms("yesterday"), None);
    assert_eq!(parse_rfc3339_ms("2026-13-01T00:00:00Z"), None);
}
